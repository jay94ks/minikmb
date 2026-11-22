//! Bounded UI input lanes: keyboard UP never waits for a mouse completion.
//! FIFO order is preserved within each interface; mouse moves coalesce only
//! within adjacent move runs. Original owner/session pinning remains shared.
use crate::{
    active::Binding,
    input::{Action, Owner},
    model::AppError,
    service::AppService,
};
use parking_lot::Mutex;
use std::{collections::VecDeque, sync::Arc};
use tokio::sync::Notify;

enum Command {
    Action(Owner, Binding, Action),
    Chord(Owner, Binding, Vec<u16>),
    Release(String),
}
impl Command {
    fn lane(&self) -> usize {
        match self {
            Self::Action(_, _, Action::Key { .. }) | Self::Chord(..) => 0,
            Self::Release(_) => 2,
            _ => 1,
        }
    }
}
#[derive(Default)]
struct Queue {
    items: VecDeque<Command>,
    owner: Option<Owner>,
    stopped: bool,
}
pub struct UiInput {
    input: std::sync::Weak<crate::input::InputManager>,
    queue: Arc<Mutex<Queue>>,
    notify: [Arc<Notify>; 3],
    errors: Arc<Mutex<Option<String>>>,
}
impl UiInput {
    pub fn new(
        app: &Arc<AppService>,
        runtime: &tokio::runtime::Handle,
        errors: Arc<Mutex<Option<String>>>,
    ) -> Self {
        let queue = Arc::new(Mutex::new(Queue::default()));
        let notify = std::array::from_fn::<_, 3, _>(|_| Arc::new(Notify::new()));
        for (index, wake) in notify.iter().enumerate() {
            let weak = Arc::downgrade(app);
            let queued = queue.clone();
            let wake = wake.clone();
            let result = errors.clone();
            runtime.spawn(async move {
                loop {
                    let next = {
                        let mut q = queued.lock();
                        if q.stopped && !q.items.iter().any(|c| c.lane() == index) {
                            break;
                        }
                        q.items
                            .iter()
                            .position(|c| c.lane() == index)
                            .and_then(|at| q.items.remove(at))
                    };
                    let Some(command) = next else {
                        wake.notified().await;
                        continue;
                    };
                    let Some(app) = weak.upgrade() else {
                        break;
                    };
                    let done = match command {
                        Command::Action(owner, b, action) => {
                            let key = matches!(action, Action::Key { .. });
                            let fields = crate::logging::Fields {
                                uid: Some(b.uid.clone()),
                                generation: Some(b.generation),
                                ..Default::default()
                            };
                            if key {
                                app.logs.emit(
                                    crate::logging::Level::Info,
                                    crate::logging::Event::KeyboardTransfer,
                                    crate::logging::Fields {
                                        code: Some("UI_KEY_APPLY"),
                                        ..fields.clone()
                                    },
                                );
                            }
                            let done = app.input.apply(owner, b, action).await.map(|_| ());
                            if key {
                                app.logs.emit(
                                    crate::logging::Level::Info,
                                    crate::logging::Event::KeyboardTransfer,
                                    crate::logging::Fields {
                                        code: Some(
                                            done.as_ref()
                                                .err()
                                                .map(|e| e.code)
                                                .unwrap_or("UI_KEY_APPLIED"),
                                        ),
                                        success: Some(done.is_ok()),
                                        ..fields
                                    },
                                );
                            }
                            done
                        }
                        Command::Chord(owner, b, keys) => {
                            app.input.chord(owner, b, keys, 30).await.map(|_| ())
                        }
                        Command::Release(id) => app.input.release_owner(id).await,
                    };
                    if let Err(e) = done {
                        if show_failure(&e) {
                            *result.lock() = Some(e.message);
                        }
                    }
                }
            });
        }
        Self {
            input: Arc::downgrade(&app.input),
            queue,
            notify,
            errors,
        }
    }
    pub fn action(&self, owner: Owner, binding: Binding, action: Action) -> Result<(), AppError> {
        action.packet()?;
        self.push(Command::Action(owner, binding, action))
    }
    pub fn chord(&self, owner: Owner, binding: Binding, keys: Vec<u16>) -> Result<(), AppError> {
        self.push(Command::Chord(owner, binding, keys))
    }
    fn push(&self, command: Command) -> Result<(), AppError> {
        let owner = match &command {
            Command::Action(o, ..) | Command::Chord(o, ..) => o.clone(),
            Command::Release(_) => unreachable!(),
        };
        let binding = match &command {
            Command::Action(_, b, _) | Command::Chord(_, b, _) => b,
            Command::Release(_) => unreachable!(),
        };
        self.input
            .upgrade()
            .ok_or_else(|| AppError::new("APP_STOPPING", "입력 서비스가 종료됐습니다."))?
            .prepare_owner(&owner, binding)?;
        let mut q = self.queue.lock();
        if q.stopped {
            return Err(AppError::new("APP_STOPPING", "입력 큐가 종료됐습니다."));
        }
        if owner.cancelled.is_cancelled() {
            return Err(AppError::new(
                "INPUT_CANCELLED",
                "입력 소유권이 종료됐습니다.",
            ));
        }
        if q.owner.as_ref().is_some_and(|o| o.id != owner.id) {
            if let Some(previous) = q.owner.take() {
                previous.cancelled.cancel();
                q.items.clear();
                q.items.push_back(Command::Release(previous.id));
            }
        }
        q.owner = Some(owner.clone());
        if coalesce(&mut q.items, &command) {
            return Ok(());
        }
        if q.items.len() >= 64 {
            owner.cancelled.cancel();
            q.items.clear();
            q.items.push_back(Command::Release(owner.id));
            q.owner = None;
            self.wake();
            *self.errors.lock() = Some("입력 큐가 가득 차 눌린 키·버튼을 해제했습니다.".into());
            return Err(AppError::new(
                "INPUT_QUEUE_FULL",
                "입력 큐가 가득 찼습니다.",
            ));
        }
        q.items.push_back(command);
        drop(q);
        self.wake();
        Ok(())
    }
    pub fn cancel(&self, owner: &Owner) {
        owner.cancelled.cancel();
        let mut q = self.queue.lock();
        if q.owner.as_ref().is_some_and(|o| o.id == owner.id) {
            q.items.clear();
            q.items.push_back(Command::Release(owner.id.clone()));
            q.owner = None;
        }
        drop(q);
        self.wake();
    }
    pub fn stop(&self) {
        let mut q = self.queue.lock();
        q.stopped = true;
        if let Some(owner) = q.owner.take() {
            owner.cancelled.cancel();
            q.items.clear();
            q.items.push_back(Command::Release(owner.id));
        }
        drop(q);
        self.wake();
    }
    fn wake(&self) {
        for notify in &self.notify {
            notify.notify_one();
        }
    }
}
/// Completion uncertainty/cancellation remains diagnostic-only. Explicitly
/// rejected or failed commands still reach the UI; API errors are unchanged.
pub fn show_failure(error: &AppError) -> bool {
    !error.uncertain
        && !matches!(
            error.code,
            "HID_UNCONFIRMED"
                | "HID_RELEASE_UNCONFIRMED"
                | "STALE_OPERATION"
                | "INPUT_CANCELLED"
                | "STALE_FRAME_REFERENCE"
                | "STALE_SESSION"
        )
}
impl Drop for UiInput {
    fn drop(&mut self) {
        self.stop();
    }
}
fn coalesce(items: &mut VecDeque<Command>, next: &Command) -> bool {
    if let (
        Some(Command::Action(old, b, Action::Move { x, y })),
        Command::Action(owner, binding, Action::Move { x: nx, y: ny }),
    ) = (items.back_mut(), next)
    {
        if old.id == owner.id && b == binding {
            old.command_guard = owner.command_guard.clone();
            *x = *nx;
            *y = *ny;
            return true;
        }
    }
    false
}
pub fn key_usage(key: eframe::egui::Key) -> Option<u16> {
    use eframe::egui::Key::*;
    Some(match key {
        A => 4,
        B => 5,
        C => 6,
        D => 7,
        E => 8,
        F => 9,
        G => 10,
        H => 11,
        I => 12,
        J => 13,
        K => 14,
        L => 15,
        M => 16,
        N => 17,
        O => 18,
        P => 19,
        Q => 20,
        R => 21,
        S => 22,
        T => 23,
        U => 24,
        V => 25,
        W => 26,
        X => 27,
        Y => 28,
        Z => 29,
        Num1 => 30,
        Num2 => 31,
        Num3 => 32,
        Num4 => 33,
        Num5 => 34,
        Num6 => 35,
        Num7 => 36,
        Num8 => 37,
        Num9 => 38,
        Num0 => 39,
        Enter => 40,
        Escape => 41,
        Backspace => 42,
        Tab => 43,
        Space => 44,
        Minus => 45,
        Equals | Plus => 46,
        OpenBracket | OpenCurlyBracket => 47,
        CloseBracket | CloseCurlyBracket => 48,
        Backslash | Pipe => 49,
        Semicolon | Colon => 51,
        Quote => 52,
        Backtick => 53,
        Comma => 54,
        Period => 55,
        Slash | Questionmark => 56,
        F1 => 58,
        F2 => 59,
        F3 => 60,
        F4 => 61,
        F5 => 62,
        F6 => 63,
        F7 => 64,
        F8 => 65,
        F9 => 66,
        F10 => 67,
        F11 => 68,
        F12 => 69,
        Insert => 73,
        Home => 74,
        PageUp => 75,
        Delete => 76,
        End => 77,
        PageDown => 78,
        ArrowRight => 79,
        ArrowLeft => 80,
        ArrowDown => 81,
        ArrowUp => 82,
        F13 => 104,
        F14 => 105,
        F15 => 106,
        F16 => 107,
        F17 => 108,
        F18 => 109,
        F19 => 110,
        F20 => 111,
        F21 => 112,
        F22 => 113,
        F23 => 114,
        F24 => 115,
        _ => return None,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use tokio_util::sync::CancellationToken;
    fn owner() -> Owner {
        Owner {
            pin: Default::default(),
            id: "ui:1".into(),
            cancelled: CancellationToken::new(),
            command_guard: None,
        }
    }
    #[tokio::test]
    async fn moves_coalesce_without_crossing_down_up_barriers() {
        let (_dir, app, wire) = crate::input_api::tests::setup();
        let b = app.active.bind().unwrap();
        let errors = Arc::new(Mutex::new(None));
        let broker = UiInput::new(&app, &tokio::runtime::Handle::current(), errors);
        let o = owner();
        broker
            .action(o.clone(), b.clone(), Action::Move { x: 1, y: 2 })
            .unwrap();
        broker
            .action(o.clone(), b.clone(), Action::Move { x: 3, y: 4 })
            .unwrap();
        broker
            .action(
                o.clone(),
                b.clone(),
                Action::Button {
                    down: true,
                    button: 1,
                },
            )
            .unwrap();
        broker
            .action(o.clone(), b.clone(), Action::Move { x: 5, y: 6 })
            .unwrap();
        broker
            .action(o.clone(), b.clone(), Action::Move { x: 7, y: 8 })
            .unwrap();
        broker
            .action(
                o,
                b,
                Action::Button {
                    down: false,
                    button: 1,
                },
            )
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while wire.calls.lock().len() < 4 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let sent = wire.calls.lock();
        assert_eq!(
            sent.iter().map(|c| c.1).collect::<Vec<_>>(),
            vec![0x103, 0x104, 0x103, 0x105]
        );
        assert_eq!(sent[0].2, vec![3, 0, 4, 0]);
        assert_eq!(sent[2].2, vec![7, 0, 8, 0]);
    }
    #[test]
    fn physical_key_map_uses_usages_not_text_characters() {
        assert_eq!(key_usage(eframe::egui::Key::A), Some(4));
        assert_eq!(key_usage(eframe::egui::Key::Escape), Some(41));
        assert_eq!(key_usage(eframe::egui::Key::ArrowLeft), Some(80));
    }
    #[test]
    fn only_confirmed_input_failures_are_visible() {
        assert!(!show_failure(&AppError::uncertain(
            "HID_UNCONFIRMED",
            "pending"
        )));
        assert!(!show_failure(&AppError::new(
            "INPUT_CANCELLED",
            "cancelled"
        )));
        assert!(!show_failure(&AppError::uncertain(
            "DEVICE_IO",
            "unknown completion"
        )));
        assert!(show_failure(&AppError::new("MCU_REJECTED", "rejected")));
        assert!(show_failure(&AppError::new(
            "INPUT_QUEUE_FULL",
            "not accepted"
        )));
    }
    #[tokio::test]
    async fn stalled_mouse_never_delays_physical_key_release() {
        let (_dir, app, wire) = crate::input_api::tests::setup();
        let b = app.active.bind().unwrap();
        let errors = Arc::new(Mutex::new(None));
        let broker = UiInput::new(&app, &tokio::runtime::Handle::current(), errors);
        let o = owner();
        wire.pause_mouse
            .store(true, std::sync::atomic::Ordering::Release);
        broker
            .action(o.clone(), b.clone(), Action::Move { x: 1, y: 2 })
            .unwrap();
        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            wire.mouse_entered.notified(),
        )
        .await
        .unwrap();
        for _ in 0..20 {
            broker
                .action(
                    o.clone(),
                    b.clone(),
                    Action::Key {
                        down: true,
                        usage: 4,
                    },
                )
                .unwrap();
            broker
                .action(
                    o.clone(),
                    b.clone(),
                    Action::Key {
                        down: false,
                        usage: 4,
                    },
                )
                .unwrap();
        }
        tokio::time::timeout(std::time::Duration::from_millis(250), async {
            loop {
                if wire
                    .calls
                    .lock()
                    .iter()
                    .filter(|c| matches!(c.1, 0x101 | 0x102))
                    .count()
                    == 40
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("key UP must finish while mouse is still blocked");
        assert_eq!(app.input.snapshot(&b).keys_held, 0);
        let keys: Vec<_> = wire
            .calls
            .lock()
            .iter()
            .filter(|c| matches!(c.1, 0x101 | 0x102))
            .map(|c| c.1)
            .collect();
        assert_eq!(keys, [0x101, 0x102].repeat(20));
        broker
            .action(
                o.clone(),
                b.clone(),
                Action::Key {
                    down: true,
                    usage: 5,
                },
            )
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_millis(250), async {
            while app.input.snapshot(&b).keys_held == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        broker.cancel(&o);
        tokio::time::timeout(std::time::Duration::from_millis(250), async {
            while app.input.snapshot(&b).keys_held != 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("focus-loss release must bypass the blocked mouse too");
        assert!(wire.calls.lock().iter().any(|c| c.1 == 0x107));
        wire.mouse_resume.notify_one();
    }
    #[tokio::test]
    async fn delayed_old_mouse_failure_cannot_release_new_keyboard_owner() {
        let (_dir, app, wire) = crate::input_api::tests::setup();
        let b = app.active.bind().unwrap();
        let old = owner();
        wire.pause_mouse
            .store(true, std::sync::atomic::Ordering::Release);
        wire.fail_mouse
            .store(true, std::sync::atomic::Ordering::Release);
        let pending = tokio::spawn({
            let input = app.input.clone();
            let b = b.clone();
            async move { input.apply(old, b, Action::Move { x: 1, y: 2 }).await }
        });
        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            wire.mouse_entered.notified(),
        )
        .await
        .unwrap();
        let mut fresh = owner();
        fresh.id = "fresh-keyboard".into();
        app.input
            .apply(
                fresh.clone(),
                b.clone(),
                Action::Key {
                    down: true,
                    usage: 4,
                },
            )
            .await
            .unwrap();
        wire.mouse_resume.notify_one();
        assert!(pending.await.unwrap().is_err());
        assert_eq!(app.input.snapshot(&b).keys_held, 1);
        assert!(!wire.calls.lock().iter().any(|c| c.1 == 0x107));
        app.input
            .apply(
                fresh,
                b.clone(),
                Action::Key {
                    down: false,
                    usage: 4,
                },
            )
            .await
            .unwrap();
        assert_eq!(app.input.snapshot(&b).keys_held, 0);
    }
}
