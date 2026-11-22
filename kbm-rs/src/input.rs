//! Shared input ownership and release recovery. Only the backend emits wire
//! commands; tests inject a recorder and never touch a real HID device.
use crate::{
    active::{ActiveCoordinator, Binding},
    actor::{DeviceHub, Scope},
    model::{AppError, Uid},
    transport::Completion,
};
use futures_util::future::BoxFuture;
use parking_lot::Mutex;
use std::{
    collections::{BTreeSet, HashMap},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, OnceLock,
    },
    time::Duration,
};
use tokio_util::sync::CancellationToken;
mod sequence;
pub use sequence::SequenceResult;
#[cfg(test)]
mod session_tests;

#[derive(Clone)]
pub struct Owner {
    pub pin: Arc<OnceLock<Pin>>,
    pub id: String,
    pub cancelled: CancellationToken,
    pub command_guard: Option<CancellationToken>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Epoch {
    pub connection: u64,
    pub session: u64,
}
pub struct Pin {
    binding: Binding,
    epoch: Epoch,
}
impl Owner {
    fn pin(&self, binding: &Binding, epoch: Epoch) -> Result<(), AppError> {
        let _ = self.pin.set(Pin {
            binding: binding.clone(),
            epoch,
        });
        self.validate_pin(binding, epoch)
    }
    pub(crate) fn validate_pin(&self, binding: &Binding, epoch: Epoch) -> Result<(), AppError> {
        if self
            .pin
            .get()
            .is_some_and(|pin| pin.binding == *binding && pin.epoch == epoch)
        {
            Ok(())
        } else {
            Err(AppError::new(
                "STALE_SESSION",
                "원래 입력의 MCU 관리 세션이 종료됐습니다.",
            ))
        }
    }
}
pub trait Backend: Send + Sync {
    /// Return immutable connection identity plus the current management wire session.
    fn epoch(&self, binding: &Binding) -> Result<Epoch, AppError>;
    fn ready(&self, binding: &Binding) -> Result<(), AppError>;
    fn send(
        &self,
        binding: Binding,
        owner: Owner,
        op: u16,
        payload: Vec<u8>,
        cleanup: bool,
    ) -> BoxFuture<'static, Result<u32, AppError>>;
}
pub struct NativeBackend {
    pub devices: Arc<DeviceHub>,
    pub logs: Arc<crate::logging::Journal>,
}
impl Backend for NativeBackend {
    fn epoch(&self, binding: &Binding) -> Result<Epoch, AppError> {
        let handle = self.devices.handle(&binding.uid)?;
        let session = handle.session();
        if session == 0 {
            return Err(AppError::new(
                "DEVICE_DISCONNECTED",
                "MCU 관리 세션이 종료됐습니다.",
            ));
        }
        Ok(Epoch {
            connection: handle.identity(),
            session,
        })
    }
    fn ready(&self, b: &Binding) -> Result<(), AppError> {
        let handle = self.devices.handle(&b.uid)?;
        let channel = handle
            .channel()
            .ok_or_else(|| AppError::new("DEVICE_DISCONNECTED", "MCU가 연결되지 않았습니다."))?;
        if channel.features & 1 == 0 {
            return Err(AppError::new(
                "HID_UNSUPPORTED",
                "현재 펌웨어는 HID를 지원하지 않습니다.",
            ));
        }
        if ![1, 2].contains(&channel.target_state) {
            return Err(AppError::new(
                "TARGET_NOT_READY",
                "타깃 USB가 입력 가능한 상태가 아닙니다.",
            ));
        }
        Ok(())
    }
    fn send(
        &self,
        binding: Binding,
        owner: Owner,
        op: u16,
        payload: Vec<u8>,
        cleanup: bool,
    ) -> BoxFuture<'static, Result<u32, AppError>> {
        let handle = self.devices.handle(&binding.uid);
        let logs = self.logs.clone();
        Box::pin(async move {
            let handle = handle?;
            owner.validate_pin(
                &binding,
                Epoch {
                    connection: handle.identity(),
                    session: handle.session(),
                },
            )?;
            let trace_key = matches!(op, 0x101 | 0x102);
            let trace_release = op == 0x107;
            let event = if trace_key {
                crate::logging::Event::KeyboardTransfer
            } else {
                crate::logging::Event::InputState
            };
            let fields = crate::logging::Fields {
                uid: Some(binding.uid.clone()),
                generation: Some(binding.generation),
                session: Some(handle.session()),
                command: Some(match op {
                    0x101 => "key_down",
                    0x107 => "hid_release",
                    _ => "key_up",
                }),
                ..Default::default()
            };
            if trace_key || trace_release {
                logs.emit(
                    crate::logging::Level::Info,
                    event,
                    crate::logging::Fields {
                        code: Some(if trace_release {
                            "MCU_RELEASE_REQUEST"
                        } else {
                            "MCU_KEY_REQUEST"
                        }),
                        ..fields.clone()
                    },
                );
            }
            let scope = if cleanup {
                Scope::Cleanup { uid: binding.uid }
            } else {
                Scope::Active {
                    binding,
                    cancel: Some(owner.cancelled),
                    guard: owner.command_guard,
                }
            };
            let result = handle
                .request(scope, 1, op, payload, Completion::HidDone)
                .await
                .and_then(|reply| {
                    reply
                        .as_slice()
                        .try_into()
                        .map(u32::from_le_bytes)
                        .map_err(|_| {
                            AppError::uncertain(
                                "INVALID_HID_TICKET",
                                "HID 완료 번호를 확인할 수 없습니다.",
                            )
                        })
                });
            if trace_key || trace_release {
                let (success, code, ticket) = match &result {
                    Ok(t) => (true, "MCU_HID_DONE", Some(*t)),
                    Err(e) => (false, e.code, None),
                };
                logs.emit(
                    if success {
                        crate::logging::Level::Info
                    } else {
                        crate::logging::Level::Warn
                    },
                    event,
                    crate::logging::Fields {
                        success: Some(success),
                        code: Some(code),
                        hid_ticket: ticket,
                        ..fields
                    },
                );
            }
            result
        })
    }
}
#[derive(Clone)]
pub enum Action {
    Key { down: bool, usage: u16 },
    Button { down: bool, button: u8 },
    Move { x: u16, y: u16 },
    Wheel { vertical: i16, horizontal: i16 },
    Release,
}
#[derive(Clone, Copy)]
pub struct Click {
    pub x: u16,
    pub y: u16,
    pub button: u8,
    pub count: u8,
    pub hold_ms: u64,
}
impl Action {
    pub fn packet(&self) -> Result<(u16, Vec<u8>), AppError> {
        let bad = || {
            AppError::new(
                "INVALID_INPUT",
                "지원하는 HID 키·버튼·좌표 범위를 확인하세요.",
            )
        };
        Ok(match self {
            Self::Key { down, usage } if valid_usage(*usage) => (
                if *down { 0x101 } else { 0x102 },
                usage.to_le_bytes().to_vec(),
            ),
            Self::Button { down, button } if (1..=5).contains(button) => {
                (if *down { 0x104 } else { 0x105 }, vec![*button])
            }
            Self::Move { x, y } if *x <= 32767 && *y <= 32767 => {
                (0x103, [x.to_le_bytes(), y.to_le_bytes()].concat())
            }
            Self::Wheel {
                vertical,
                horizontal,
            } if (-127..=127).contains(vertical) && (-127..=127).contains(horizontal) => (
                0x106,
                [vertical.to_le_bytes(), horizontal.to_le_bytes()].concat(),
            ),
            Self::Release => (0x107, Vec::new()),
            _ => return Err(bad()),
        })
    }
}
fn valid_usage(usage: u16) -> bool {
    (4..=0xa4).contains(&usage) || (0xe0..=0xe7).contains(&usage)
}
struct Held {
    owner: Owner,
    binding: Binding,
    keys: BTreeSet<u16>,
    buttons: BTreeSet<u8>,
}
impl Held {
    fn empty(&self) -> bool {
        self.keys.is_empty() && self.buttons.is_empty()
    }
}
#[derive(Default)]
struct State {
    held: Option<Held>,
    blocked: HashMap<Uid, String>,
}
impl State {
    // Bounded diagnostic history, never a permanent interlock on later work.
    fn release_error(&mut self, uid: Uid, message: String) {
        if self.blocked.len() >= 128 && !self.blocked.contains_key(&uid) {
            if let Some(old) = self.blocked.keys().next().cloned() {
                self.blocked.remove(&old);
            }
        }
        self.blocked.insert(uid, message);
    }
}
#[derive(Clone, Default, serde::Serialize, schemars::JsonSchema)]
pub struct InputStatus {
    pub ready: bool,
    pub keys_held: usize,
    pub buttons_held: usize,
    pub release_unconfirmed: bool,
    pub error: String,
}
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct InputResult {
    pub usb_completed: bool,
    pub no_op: bool,
    pub ticket: Option<u32>,
}
pub struct InputManager {
    logger: Mutex<Option<std::sync::Weak<crate::logging::Journal>>>,
    active: Arc<ActiveCoordinator>,
    backend: Arc<dyn Backend>,
    gate: tokio::sync::Mutex<()>,
    keyboard_gate: tokio::sync::Mutex<()>,
    cleanup_gate: tokio::sync::Mutex<()>,
    state: Mutex<State>,
    stopping: AtomicBool,
    pending: Arc<tokio::sync::Semaphore>,
}
impl InputManager {
    pub fn new(active: Arc<ActiveCoordinator>, backend: Arc<dyn Backend>) -> Arc<Self> {
        Arc::new(Self {
            logger: Default::default(),
            active,
            backend,
            gate: Default::default(),
            keyboard_gate: Default::default(),
            cleanup_gate: Default::default(),
            state: Default::default(),
            stopping: AtomicBool::new(false),
            pending: Arc::new(tokio::sync::Semaphore::new(64)),
        })
    }
    pub fn snapshot(&self, binding: &Binding) -> InputStatus {
        let state = self.state.lock();
        let held = state.held.as_ref().filter(|h| h.binding.uid == binding.uid);
        InputStatus {
            ready: self.active.validate(binding).is_ok() && self.backend.ready(binding).is_ok(),
            keys_held: held.map_or(0, |h| h.keys.len()),
            buttons_held: held.map_or(0, |h| h.buttons.len()),
            release_unconfirmed: state.blocked.contains_key(&binding.uid),
            error: state.blocked.get(&binding.uid).cloned().unwrap_or_default(),
        }
    }
    pub fn set_logger(&self, logger: &Arc<crate::logging::Journal>) {
        *self.logger.lock() = Some(Arc::downgrade(logger));
    }
    pub fn prepare_owner(&self, owner: &Owner, binding: &Binding) -> Result<(), AppError> {
        if owner.cancelled.is_cancelled() {
            return Err(AppError::new(
                "INPUT_CANCELLED",
                "입력 소유권이 종료됐습니다.",
            ));
        }
        if owner
            .command_guard
            .as_ref()
            .is_some_and(CancellationToken::is_cancelled)
        {
            return Err(AppError::new(
                "STALE_FRAME_REFERENCE",
                "원래 영상 스트림이 종료됐습니다.",
            ));
        }
        self.active.validate(binding)?;
        let result = self
            .backend
            .epoch(binding)
            .and_then(|epoch| owner.pin(binding, epoch));
        if result.is_err() {
            owner.cancelled.cancel();
            let mut state = self.state.lock();
            if state
                .held
                .as_ref()
                .is_some_and(|held| held.owner.id == owner.id && !held.empty())
            {
                state.release_error(
                    binding.uid.clone(),
                    "MCU 관리 세션 변경으로 입력 해제를 확인하지 못했습니다.".into(),
                );
                state.held = None;
            }
        }
        result.map_err(|e| e.at(binding))
    }
    async fn send_pinned(
        &self,
        binding: Binding,
        owner: Owner,
        op: u16,
        payload: Vec<u8>,
        cleanup: bool,
    ) -> Result<u32, AppError> {
        owner.validate_pin(&binding, self.backend.epoch(&binding)?)?;
        self.backend
            .send(binding, owner, op, payload, cleanup)
            .await
    }
    pub async fn apply(
        self: &Arc<Self>,
        owner: Owner,
        binding: Binding,
        action: Action,
    ) -> Result<InputResult, AppError> {
        let original = binding.clone();
        let command = match &action {
            Action::Key { down: true, .. } => "hid_key_down",
            Action::Key { down: false, .. } => "hid_key_up",
            Action::Button { down: true, .. } => "hid_button_down",
            Action::Button { down: false, .. } => "hid_button_up",
            Action::Move { .. } => "hid_mouse_move",
            Action::Wheel { .. } => "hid_wheel",
            Action::Release => "hid_release",
        };
        let result = self
            .apply_inner(owner, binding, action)
            .await
            .map_err(|e| e.at(&original));
        if let Some(logger) = self
            .logger
            .lock()
            .as_ref()
            .and_then(std::sync::Weak::upgrade)
        {
            logger.command(command, Some(&original), &result);
        }
        result
    }
    async fn apply_inner(
        self: &Arc<Self>,
        owner: Owner,
        binding: Binding,
        action: Action,
    ) -> Result<InputResult, AppError> {
        action.packet()?;
        self.prepare_owner(&owner, &binding)?;
        let permit = self.reserve()?;
        let manager = self.clone();
        // A disconnected requester cannot abandon an already-emitted DOWN.
        tokio::spawn(async move {
            let _permit = permit;
            // An unpolled mouse must not delay a physical keyboard UP.
            let gate = if matches!(action, Action::Key { .. }) {
                &manager.keyboard_gate
            } else {
                &manager.gate
            };
            let _gate = gate.lock().await;
            manager.apply_locked(owner, binding, action).await
        })
        .await
        .map_err(|_| AppError::uncertain("INPUT_FAILED", "입력 처리 작업이 종료됐습니다."))?
    }
    fn reserve(&self) -> Result<tokio::sync::OwnedSemaphorePermit, AppError> {
        self.pending
            .clone()
            .try_acquire_owned()
            .map_err(|_| AppError::new("INPUT_QUEUE_FULL", "대기 중인 입력 명령이 너무 많습니다."))
    }
    fn claim(&self, owner: &Owner, b: &Binding) -> Result<(), AppError> {
        self.prepare_owner(owner, b)?;
        if self.stopping.load(Ordering::Acquire) {
            return Err(AppError::new("APP_STOPPING", "앱이 종료 중입니다."));
        }
        self.active.validate(b)?;
        if owner
            .command_guard
            .as_ref()
            .is_some_and(|g| g.is_cancelled())
        {
            return Err(AppError::new(
                "STALE_FRAME_REFERENCE",
                "영상 스트림이 변경돼 좌표 입력을 취소했습니다.",
            ));
        }
        if owner.cancelled.is_cancelled() {
            return Err(AppError::new(
                "INPUT_CANCELLED",
                "입력 소유권이 종료됐습니다.",
            ));
        }
        self.backend.ready(b)?;
        let mut state = self.state.lock();
        if state.held.as_ref().is_some_and(|h| {
            !h.empty()
                && (h.owner.cancelled.is_cancelled() || h.owner.id != owner.id || h.binding != *b)
        }) {
            return Err(AppError::new(
                "INPUT_BUSY",
                "다른 입력 소유자가 키 또는 버튼을 누르고 있습니다.",
            ));
        }
        if state.held.as_ref().is_none_or(|h| h.empty()) {
            state.held = Some(Held {
                owner: owner.clone(),
                binding: b.clone(),
                keys: Default::default(),
                buttons: Default::default(),
            });
        }
        Ok(())
    }
    async fn apply_locked(
        &self,
        owner: Owner,
        binding: Binding,
        action: Action,
    ) -> Result<InputResult, AppError> {
        if matches!(action, Action::Release) {
            self.active.validate(&binding)?;
            let had_work = {
                let state = self.state.lock();
                state
                    .held
                    .as_ref()
                    .is_some_and(|h| h.owner.id == owner.id && !h.empty())
            };
            self.release_owner_locked(&owner.id).await?;
            return Ok(InputResult {
                usb_completed: had_work,
                no_op: !had_work,
                ticket: None,
            });
        }
        self.claim(&owner, &binding)?;
        let (op, payload) = action.packet()?;
        let changed = {
            let mut state = self.state.lock();
            let release_was_unconfirmed = state.blocked.contains_key(&binding.uid);
            let held = state.held.as_mut().unwrap();
            match &action {
                Action::Key { down: true, usage } => {
                    if *usage < 0xe0
                        && !held.keys.contains(usage)
                        && held.keys.iter().filter(|u| **u < 0xe0).count() >= 6
                    {
                        return Err(AppError::new(
                            "KEY_ROLLOVER",
                            "동시에 누를 수 있는 일반 키는 6개입니다.",
                        ));
                    }
                    held.keys.insert(*usage)
                }
                Action::Key { down: false, usage } => {
                    // A concurrent release may clear bookkeeping before this
                    // physical UP. Always send it to the pinned original MCU.
                    held.keys.remove(usage);
                    true
                }
                Action::Button { down: true, button } => held.buttons.insert(*button),
                Action::Button {
                    down: false,
                    button,
                } => held.buttons.remove(button) || release_was_unconfirmed,
                _ => true,
            }
        };
        if !changed {
            return Ok(InputResult {
                usb_completed: false,
                no_op: true,
                ticket: None,
            });
        }
        match self
            .send_pinned(binding.clone(), owner.clone(), op, payload, false)
            .await
        {
            Ok(_) if owner.cancelled.is_cancelled() => {
                let has_held =
                    self.state.lock().held.as_ref().is_some_and(|h| {
                        h.binding == binding && h.owner.id == owner.id && !h.empty()
                    });
                if has_held {
                    let _ = self.recover_source_locked(binding, owner.clone()).await;
                }
                Err(AppError::uncertain(
                    "INPUT_CANCELLED",
                    "입력 소유권 종료로 원래 채널의 입력을 해제했습니다.",
                ))
            }
            Ok(ticket) => Ok(InputResult {
                usb_completed: true,
                no_op: false,
                ticket: Some(ticket),
            }),
            Err(error) => {
                if !error.uncertain {
                    // An explicit negative ACK or pre-send rejection did not
                    // execute this transition; restore local bookkeeping.
                    let mut state = self.state.lock();
                    if let Some(h) = state
                        .held
                        .as_mut()
                        .filter(|h| h.binding == binding && h.owner.id == owner.id)
                    {
                        match action {
                            Action::Key { down: true, usage } => {
                                h.keys.remove(&usage);
                            }
                            Action::Key { down: false, usage } => {
                                h.keys.insert(usage);
                            }
                            Action::Button { down: true, button } => {
                                h.buttons.remove(&button);
                            }
                            Action::Button {
                                down: false,
                                button,
                            } => {
                                h.buttons.insert(button);
                            }
                            _ => {}
                        }
                    }
                }
                // UP may have failed after local bookkeeping removed the last
                // key; always recover the original MCU, never the new active.
                let has_held =
                    self.state.lock().held.as_ref().is_some_and(|h| {
                        h.binding == binding && h.owner.id == owner.id && !h.empty()
                    });
                // A move/wheel cannot leave a key or button pressed. A missing
                // completion for either must not invent release debt and lock
                // channel selection. An uncertain UP still needs cleanup even
                // though local bookkeeping has already removed that input.
                let changes_held = matches!(action, Action::Key { .. } | Action::Button { .. });
                if (error.uncertain && changes_held) || has_held {
                    let _ = self.recover_source_locked(binding, owner.clone()).await;
                }
                Err(error)
            }
        }
    }
    async fn recover_locked(&self, binding: Binding) -> Result<(), AppError> {
        let owner = self
            .state
            .lock()
            .held
            .as_ref()
            .filter(|h| h.binding.uid == binding.uid)
            .map(|h| h.owner.clone())
            .ok_or_else(|| {
                AppError::uncertain(
                    "HID_RELEASE_UNCONFIRMED",
                    "원래 입력의 관리 세션을 확인하지 못했습니다.",
                )
            })?;
        self.recover_source_locked(binding, owner).await
    }
    async fn recover_source_locked(&self, binding: Binding, owner: Owner) -> Result<(), AppError> {
        // Independent lanes can finish after an empty old owner was replaced.
        // Its delayed failure must not release a new owner's pressed inputs.
        if self
            .state
            .lock()
            .held
            .as_ref()
            .is_some_and(|h| h.binding.uid == binding.uid && h.owner.id != owner.id && !h.empty())
        {
            return Err(AppError::uncertain(
                "HID_RELEASE_UNCONFIRMED",
                "이전 입력 소유자의 정리를 새 입력에 적용하지 않았습니다.",
            )
            .at(&binding));
        }
        let mut result = Err(AppError::uncertain(
            "HID_RELEASE_UNCONFIRMED",
            "입력 해제 완료를 확인하지 못했습니다.",
        ));
        for attempt in 0..5 {
            result = self
                .send_pinned(binding.clone(), owner.clone(), 0x107, Vec::new(), true)
                .await
                .map(|_| ());
            if result.as_ref().err().is_some_and(|e| e.status == Some(4)) && attempt < 4 {
                tokio::time::sleep(Duration::from_millis(20)).await;
            } else {
                break;
            }
        }
        let mut state = self.state.lock();
        match result {
            Ok(()) => {
                state.blocked.remove(&binding.uid);
                if state
                    .held
                    .as_ref()
                    .is_some_and(|h| h.binding.uid == binding.uid && h.owner.id == owner.id)
                {
                    state.held = None;
                }
                Ok(())
            }
            Err(_) => {
                state.release_error(binding.uid.clone(), "입력 해제 완료 미확인".into());
                // The release attempt remains unconfirmed in diagnostics. Do
                // not keep the expired owner latched or require a manual ACK
                // before a new user action or a channel change can proceed.
                if state
                    .held
                    .as_ref()
                    .is_some_and(|h| h.binding.uid == binding.uid && h.owner.id == owner.id)
                {
                    state.held = None;
                }
                Err(AppError::uncertain(
                    "HID_RELEASE_UNCONFIRMED",
                    "원래 MCU의 입력 해제 완료를 확인하지 못했습니다.",
                )
                .at(&binding))
            }
        }
    }
    async fn release_owner_locked(&self, id: &str) -> Result<(), AppError> {
        let binding = {
            let state = self.state.lock();
            state
                .held
                .as_ref()
                .filter(|h| h.owner.id == id && !h.empty())
                .map(|h| h.binding.clone())
        };
        if let Some(b) = binding {
            self.recover_locked(b).await?;
        }
        Ok(())
    }
    pub async fn release_owner(self: &Arc<Self>, id: String) -> Result<(), AppError> {
        let manager = self.clone();
        tokio::spawn(async move {
            let _gate = manager.cleanup_gate.lock().await;
            manager.release_owner_locked(&id).await
        })
        .await
        .map_err(|_| {
            AppError::uncertain("HID_RELEASE_UNCONFIRMED", "입력 해제 작업이 종료됐습니다.")
        })?
    }
    pub async fn release_channel(&self, uid: &Uid) -> Result<(), AppError> {
        let _gate = self.cleanup_gate.lock().await;
        let binding = {
            let state = self.state.lock();
            state
                .held
                .as_ref()
                .filter(|h| &h.binding.uid == uid && !h.empty())
                .map(|h| h.binding.clone())
        };
        if let Some(b) = binding {
            self.recover_locked(b).await?;
        }
        Ok(())
    }
    pub async fn recover(self: &Arc<Self>, binding: Binding) -> Result<(), AppError> {
        self.active.validate(&binding)?;
        let owner = Owner {
            pin: Default::default(),
            id: "release-recovery".into(),
            cancelled: CancellationToken::new(),
            command_guard: None,
        };
        owner.pin(&binding, self.backend.epoch(&binding)?)?;
        let manager = self.clone();
        tokio::spawn(async move {
            let _gate = manager.cleanup_gate.lock().await;
            manager.active.validate(&binding)?;
            manager.recover_source_locked(binding, owner).await
        })
        .await
        .map_err(|_| {
            AppError::uncertain("HID_RELEASE_UNCONFIRMED", "입력 해제 작업이 종료됐습니다.")
        })?
    }
    pub async fn expire_cancelled(&self) {
        let _gate = self.cleanup_gate.lock().await;
        let binding = {
            let state = self.state.lock();
            state
                .held
                .as_ref()
                .filter(|h| {
                    (h.owner.cancelled.is_cancelled()
                        || h.owner
                            .command_guard
                            .as_ref()
                            .is_some_and(|g| g.is_cancelled()))
                        && !h.empty()
                })
                .map(|h| h.binding.clone())
        };
        if let Some(b) = binding {
            let _ = self.recover_locked(b).await;
        }
    }
    pub fn disconnected(&self, uid: &Uid) {
        let mut state = self.state.lock();
        if state.held.as_ref().is_some_and(|h| &h.binding.uid == uid) {
            if state.held.as_ref().is_some_and(|h| !h.empty()) {
                state.release_error(
                    uid.clone(),
                    "연결 종료로 입력 해제를 확인하지 못했습니다.".into(),
                );
            }
            state.held = None;
        }
    }
    pub fn stop(&self) {
        self.stopping.store(true, Ordering::Release);
    }
    pub async fn chord(
        self: &Arc<Self>,
        owner: Owner,
        binding: Binding,
        usages: Vec<u16>,
        hold_ms: u64,
    ) -> Result<InputResult, AppError> {
        let original = binding.clone();
        let result = self
            .chord_inner(owner, binding, usages, hold_ms)
            .await
            .map_err(|e| e.at(&original));
        if let Some(logger) = self
            .logger
            .lock()
            .as_ref()
            .and_then(std::sync::Weak::upgrade)
        {
            logger.command("hid_chord", Some(&original), &result);
        }
        result
    }
    async fn chord_inner(
        self: &Arc<Self>,
        owner: Owner,
        binding: Binding,
        usages: Vec<u16>,
        hold_ms: u64,
    ) -> Result<InputResult, AppError> {
        self.prepare_owner(&owner, &binding)?;
        let unique: BTreeSet<_> = usages.iter().copied().collect();
        if unique.is_empty()
            || unique.len() != usages.len()
            || usages.iter().any(|u| !valid_usage(*u))
            || unique.iter().filter(|u| **u < 0xe0).count() > 6
            || hold_ms > 1000
        {
            return Err(AppError::new(
                "INVALID_INPUT",
                "키 조합과 누름 시간(최대 1초)을 확인하세요.",
            ));
        }
        let manager = self.clone();
        let permit = self.reserve()?;
        tokio::spawn(async move {
            let _permit=permit;
            let _gate=manager.keyboard_gate.lock().await;
            manager.claim(&owner,&binding)?;
            manager.release_owner_locked(&owner.id).await?;
            let mut failure=None;
            let mut attempted=Vec::new();
            for usage in &usages {
                attempted.push(*usage);
                if let Err(e)=manager.apply_locked(owner.clone(),binding.clone(),Action::Key {down:true,usage:*usage}).await {failure=Some(e);break;}
            }
            if failure.is_none() {
                tokio::select! {_=tokio::time::sleep(Duration::from_millis(hold_ms))=>{},_=owner.cancelled.cancelled()=>{failure=Some(AppError::uncertain("INPUT_CANCELLED","키 조합 전송 중 입력 소유권이 종료됐습니다."));}}
            }
            // Reverse UP attempts survive lease cancellation or active switch.
            // Only release operations are permitted in this cleanup scope.
            for usage in attempted.into_iter().rev() {
                match manager.send_pinned(binding.clone(),owner.clone(),0x102,usage.to_le_bytes().to_vec(),true).await {
                    Ok(_)=>{let mut state=manager.state.lock();if let Some(h)=state.held.as_mut().filter(|h|h.binding==binding&&h.owner.id==owner.id){h.keys.remove(&usage);}},
                    Err(e)=>{if failure.is_none(){failure=Some(e);}}
                }
            }
            if let Some(e)=failure {let _=manager.recover_locked(binding).await;return Err(e);}
            Ok(InputResult {usb_completed:true,no_op:false,ticket:None})
        }).await.map_err(|_|AppError::uncertain("INPUT_FAILED","키 조합 처리 작업이 종료됐습니다."))?
    }
    pub async fn click(
        self: &Arc<Self>,
        owner: Owner,
        binding: Binding,
        click: Click,
    ) -> Result<InputResult, AppError> {
        let original = binding.clone();
        let result = self
            .click_inner(owner, binding, click)
            .await
            .map_err(|e| e.at(&original));
        if let Some(logger) = self
            .logger
            .lock()
            .as_ref()
            .and_then(std::sync::Weak::upgrade)
        {
            logger.command("hid_click", Some(&original), &result);
        }
        result
    }
    async fn click_inner(
        self: &Arc<Self>,
        owner: Owner,
        binding: Binding,
        click: Click,
    ) -> Result<InputResult, AppError> {
        self.prepare_owner(&owner, &binding)?;
        let Click {
            x,
            y,
            button,
            count,
            hold_ms,
        } = click;
        Action::Move { x, y }.packet()?;
        Action::Button { down: true, button }.packet()?;
        if !(1..=2).contains(&count) || hold_ms > 1000 {
            return Err(AppError::new(
                "INVALID_INPUT",
                "클릭 횟수(1~2)와 누름 시간(최대1초)을 확인하세요.",
            ));
        }
        let manager = self.clone();
        let permit = self.reserve()?;
        tokio::spawn(async move {
            let _permit=permit;
            let _gate=manager.gate.lock().await;manager.claim(&owner,&binding)?;
            if manager.state.lock().held.as_ref().is_some_and(|h|h.buttons.contains(&button)){return Err(AppError::new("INPUT_BUSY","이미 누른 버튼으로 클릭을 시작할 수 없습니다."));}
            manager.apply_locked(owner.clone(),binding.clone(),Action::Move{x,y}).await?;
            let mut ticket=None;
            for n in 0..count {
                manager.apply_locked(owner.clone(),binding.clone(),Action::Button{down:true,button}).await?;
                tokio::select!{_=tokio::time::sleep(Duration::from_millis(hold_ms))=>{},_=owner.cancelled.cancelled()=>{}}
                match manager.send_pinned(binding.clone(),owner.clone(),0x105,vec![button],true).await {
                    Ok(t)=>{ticket=Some(t);let mut state=manager.state.lock();if let Some(h)=state.held.as_mut().filter(|h|h.binding==binding&&h.owner.id==owner.id){h.buttons.remove(&button);}},
                    Err(e)=>{let _=manager.recover_locked(binding).await;return Err(e);}
                }
                if owner.cancelled.is_cancelled(){let _=manager.recover_locked(binding.clone()).await;return Err(AppError::uncertain("INPUT_CANCELLED","클릭 중 입력 소유권이 종료돼 버튼을 해제했습니다."));}
                if n+1<count {tokio::time::sleep(Duration::from_millis(100)).await;}
            }
            Ok(InputResult{usb_completed:true,no_op:false,ticket})
        }).await.map_err(|_|AppError::uncertain("INPUT_FAILED","클릭 처리 작업이 종료됐습니다."))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    type Sent = Vec<(Uid, u16, Vec<u8>, bool)>;
    #[derive(Default)]
    struct Mock {
        sent: Mutex<Sent>,
        replies: Mutex<std::collections::VecDeque<Result<u32, AppError>>>,
    }
    impl Backend for Mock {
        fn epoch(&self, _: &Binding) -> Result<Epoch, AppError> {
            Ok(Epoch::default())
        }
        fn ready(&self, _: &Binding) -> Result<(), AppError> {
            Ok(())
        }
        fn send(
            &self,
            b: Binding,
            _: Owner,
            op: u16,
            payload: Vec<u8>,
            cleanup: bool,
        ) -> BoxFuture<'static, Result<u32, AppError>> {
            self.sent.lock().push((b.uid, op, payload, cleanup));
            let result = self.replies.lock().pop_front().unwrap_or(Ok(1));
            Box::pin(async move { result })
        }
    }
    fn setup() -> (Arc<InputManager>, Arc<Mock>, Binding, Owner) {
        let active = Arc::new(ActiveCoordinator::default());
        active.begin().unwrap();
        active.finish(Some(Uid::from_bytes(&[1; 12]).unwrap()));
        let b = active.bind().unwrap();
        let backend = Arc::new(Mock::default());
        let manager = InputManager::new(active, backend.clone());
        (
            manager,
            backend,
            b,
            Owner {
                pin: Default::default(),
                id: "client-A".into(),
                cancelled: CancellationToken::new(),
                command_guard: None,
            },
        )
    }
    #[tokio::test]
    async fn owners_cannot_interleave_pressed_inputs_and_cancel_releases_only_original_uid() {
        let (m, wire, b, a) = setup();
        m.apply(
            a.clone(),
            b.clone(),
            Action::Key {
                down: true,
                usage: 4,
            },
        )
        .await
        .unwrap();
        let other = Owner {
            pin: Default::default(),
            id: "client-B".into(),
            cancelled: CancellationToken::new(),
            command_guard: None,
        };
        assert_eq!(
            m.apply(
                other,
                b.clone(),
                Action::Key {
                    down: true,
                    usage: 5
                }
            )
            .await
            .err()
            .unwrap()
            .code,
            "INPUT_BUSY"
        );
        a.cancelled.cancel();
        m.expire_cancelled().await;
        let sent = wire.sent.lock();
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[0].1, 0x101);
        assert_eq!(sent[1].1, 0x107);
        assert_eq!(sent[1].0, b.uid);
        assert!(sent[1].3);
        assert_eq!(m.snapshot(&b).keys_held, 0);
    }
    #[tokio::test]
    async fn uncertain_down_is_not_repeated_and_failed_release_does_not_latch_future_input() {
        let (m, wire, b, a) = setup();
        wire.replies.lock().extend([
            Err(AppError::uncertain("HID_UNCONFIRMED", "test")),
            Err(AppError::uncertain("DEVICE_IO", "test")),
        ]);
        assert!(m
            .apply(
                a.clone(),
                b.clone(),
                Action::Key {
                    down: true,
                    usage: 4
                }
            )
            .await
            .is_err());
        assert!(m.snapshot(&b).release_unconfirmed);
        assert!(m.snapshot(&b).ready);
        assert_eq!(wire.sent.lock().iter().filter(|s| s.1 == 0x101).count(), 1);
        m.release_channel(&b.uid).await.unwrap();
        // A later physical UP still reaches the same MCU after failed cleanup.
        m.apply(
            a.clone(),
            b.clone(),
            Action::Key {
                down: false,
                usage: 4,
            },
        )
        .await
        .unwrap();
        assert_eq!(wire.sent.lock().last().unwrap().1, 0x102);
        // A new explicit input is allowed; the failed original DOWN is never retried.
        m.apply(
            a,
            b,
            Action::Key {
                down: true,
                usage: 5,
            },
        )
        .await
        .unwrap();
        let sent = wire.sent.lock();
        assert_eq!(
            sent.iter()
                .filter(|s| s.1 == 0x101 && s.2 == [4, 0])
                .count(),
            1
        );
        assert_eq!(sent.last().unwrap().2, [5, 0]);
    }
    #[tokio::test]
    async fn explicit_negative_ack_does_not_invent_a_pressed_key_or_unnecessary_release() {
        let (m, wire, b, a) = setup();
        wire.replies.lock().push_back(Err(AppError::mcu(9)));
        let error = m
            .apply(
                a,
                b.clone(),
                Action::Key {
                    down: true,
                    usage: 4,
                },
            )
            .await
            .err()
            .unwrap();
        assert_eq!(error.status, Some(9));
        assert!(!error.uncertain);
        assert_eq!(m.snapshot(&b).keys_held, 0);
        assert!(!m.snapshot(&b).release_unconfirmed);
        assert_eq!(wire.sent.lock().len(), 1);
    }
    #[tokio::test]
    async fn unconfirmed_motion_does_not_invent_pressed_input_or_block_channel_switch() {
        for action in [
            Action::Move { x: 120, y: 240 },
            Action::Wheel {
                vertical: 1,
                horizontal: 0,
            },
        ] {
            let (m, wire, b, a) = setup();
            wire.replies
                .lock()
                .push_back(Err(AppError::uncertain("HID_UNCONFIRMED", "test")));
            assert_eq!(
                m.apply(a, b.clone(), action).await.err().unwrap().code,
                "HID_UNCONFIRMED"
            );
            assert!(!m.snapshot(&b).release_unconfirmed);
            assert_eq!(wire.sent.lock().len(), 1); // No unnecessary RELEASE_ALL.
            m.active.begin().unwrap();
            m.release_channel(&b.uid).await.unwrap();
            m.active.finish(Some(Uid::from_bytes(&[2; 12]).unwrap()));
            assert_eq!(
                m.active.bind().unwrap().uid,
                Uid::from_bytes(&[2; 12]).unwrap()
            );
        }
    }
    #[tokio::test]
    async fn motion_failure_during_drag_still_recovers_the_original_button() {
        let (m, wire, b, a) = setup();
        m.apply(
            a.clone(),
            b.clone(),
            Action::Button {
                down: true,
                button: 1,
            },
        )
        .await
        .unwrap();
        wire.replies.lock().extend([
            Err(AppError::uncertain("HID_UNCONFIRMED", "test")),
            Err(AppError::uncertain("DEVICE_IO", "test")),
        ]);
        assert!(m
            .apply(a, b.clone(), Action::Move { x: 120, y: 240 })
            .await
            .is_err());
        assert!(m.snapshot(&b).release_unconfirmed);
        let sent = wire.sent.lock();
        assert_eq!(
            sent.iter().map(|s| s.1).collect::<Vec<_>>(),
            vec![0x104, 0x103, 0x107]
        );
        assert_eq!(sent.last().unwrap().0, b.uid);
        assert!(sent.last().unwrap().3);
    }
    #[tokio::test]
    async fn uncertain_final_up_is_not_treated_as_empty_confirmed_state() {
        let (m, wire, b, a) = setup();
        m.apply(
            a.clone(),
            b.clone(),
            Action::Key {
                down: true,
                usage: 4,
            },
        )
        .await
        .unwrap();
        wire.replies.lock().extend([
            Err(AppError::uncertain("HID_UNCONFIRMED", "test")),
            Err(AppError::uncertain("DEVICE_IO", "test")),
        ]);
        assert!(m
            .apply(
                a,
                b.clone(),
                Action::Key {
                    down: false,
                    usage: 4
                }
            )
            .await
            .is_err());
        assert_eq!(m.snapshot(&b).keys_held, 0);
        assert!(m.snapshot(&b).release_unconfirmed);
        assert_eq!(wire.sent.lock().last().unwrap().1, 0x107);
    }
    #[tokio::test]
    async fn old_release_history_never_blocks_a_fresh_virtual_chord_or_its_later_cleanup() {
        let (m, wire, b, old) = setup();
        wire.replies.lock().extend([
            Err(AppError::uncertain("HID_UNCONFIRMED", "test")),
            Err(AppError::uncertain("DEVICE_IO", "test")),
        ]);
        assert!(m
            .apply(
                old,
                b.clone(),
                Action::Key {
                    down: true,
                    usage: 4
                }
            )
            .await
            .is_err());
        assert!(m.snapshot(&b).release_unconfirmed);
        let fresh = Owner {
            pin: Default::default(),
            id: "fresh-virtual-key".into(),
            cancelled: CancellationToken::new(),
            command_guard: None,
        };
        m.chord(fresh.clone(), b.clone(), vec![5], 0).await.unwrap();
        assert_eq!(
            wire.sent.lock().iter().map(|c| c.1).collect::<Vec<_>>(),
            vec![0x101, 0x107, 0x101, 0x102]
        );
        m.apply(
            fresh.clone(),
            b.clone(),
            Action::Key {
                down: true,
                usage: 6,
            },
        )
        .await
        .unwrap();
        fresh.cancelled.cancel();
        m.expire_cancelled().await;
        assert_eq!(wire.sent.lock().last().unwrap().1, 0x107);
        assert!(!m.snapshot(&b).release_unconfirmed);
        assert_eq!(m.snapshot(&b).keys_held, 0);
    }
    #[tokio::test]
    async fn chord_and_switch_cleanup_stay_pinned_and_no_key_values_are_serialized() {
        let (m, wire, b, a) = setup();
        m.chord(a.clone(), b.clone(), vec![0xe0, 4], 0)
            .await
            .unwrap();
        assert_eq!(
            wire.sent.lock().iter().map(|s| s.1).collect::<Vec<_>>(),
            vec![0x101, 0x101, 0x102, 0x102]
        );
        m.apply(
            a,
            b.clone(),
            Action::Button {
                down: true,
                button: 1,
            },
        )
        .await
        .unwrap();
        m.active.begin().unwrap();
        m.release_channel(&b.uid).await.unwrap();
        m.active.finish(Some(Uid::from_bytes(&[2; 12]).unwrap()));
        assert_eq!(wire.sent.lock().last().unwrap().0, b.uid);
        let json = serde_json::to_value(m.snapshot(&m.active.bind().unwrap())).unwrap();
        assert!(json.get("keys").is_none());
        assert!(json.get("usages").is_none());
    }
    #[test]
    fn wire_packets_match_v1_ranges_and_endianness() {
        assert_eq!(
            Action::Key {
                down: true,
                usage: 0xe7
            }
            .packet()
            .unwrap(),
            (0x101, vec![0xe7, 0])
        );
        assert_eq!(
            Action::Move { x: 32767, y: 0 }.packet().unwrap(),
            (0x103, vec![255, 127, 0, 0])
        );
        assert_eq!(
            Action::Wheel {
                vertical: -1,
                horizontal: 2
            }
            .packet()
            .unwrap(),
            (0x106, vec![255, 255, 2, 0])
        );
        assert!(Action::Key {
            down: true,
            usage: 0xb0
        }
        .packet()
        .is_err());
        assert!(Action::Button {
            down: true,
            button: 0
        }
        .packet()
        .is_err());
    }
}
