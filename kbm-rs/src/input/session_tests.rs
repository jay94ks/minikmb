use super::*;
use crate::{input_sessions::Principal, service::AppService};
use std::sync::atomic::AtomicU64;
use tokio::sync::Notify;
#[derive(Clone)]
struct Call {
    epoch: Epoch,
    op: u16,
    cleanup: bool,
}
struct Wire {
    connection: AtomicU64,
    session: AtomicU64,
    calls: Arc<Mutex<Vec<Call>>>,
    pause: AtomicBool,
    entered: Notify,
    resume: Notify,
    sampled: Notify,
}
impl Default for Wire {
    fn default() -> Self {
        Self {
            connection: AtomicU64::new(1),
            session: AtomicU64::new(7),
            calls: Default::default(),
            pause: AtomicBool::new(false),
            entered: Notify::new(),
            resume: Notify::new(),
            sampled: Notify::new(),
        }
    }
}
impl Wire {
    fn current(&self) -> Epoch {
        Epoch {
            connection: self.connection.load(Ordering::Acquire),
            session: self.session.load(Ordering::Acquire),
        }
    }
}
struct BackendWire(Arc<Wire>);
impl Backend for BackendWire {
    fn epoch(&self, _: &Binding) -> Result<Epoch, AppError> {
        self.0.sampled.notify_one();
        Ok(self.0.current())
    }
    fn ready(&self, _: &Binding) -> Result<(), AppError> {
        Ok(())
    }
    fn send(
        &self,
        b: Binding,
        owner: Owner,
        op: u16,
        _payload: Vec<u8>,
        cleanup: bool,
    ) -> BoxFuture<'static, Result<u32, AppError>> {
        let wire = self.0.clone();
        Box::pin(async move {
            if wire.pause.swap(false, Ordering::AcqRel) {
                wire.entered.notify_one();
                wire.resume.notified().await;
            }
            let epoch = wire.current();
            owner.validate_pin(&b, epoch)?;
            wire.calls.lock().push(Call { epoch, op, cleanup });
            Ok(1)
        })
    }
}
fn owner() -> Owner {
    Owner {
        pin: Default::default(),
        id: "old-client".into(),
        cancelled: CancellationToken::new(),
        command_guard: None,
    }
}
fn setup() -> (tempfile::TempDir, Arc<AppService>, Arc<Wire>) {
    let (dir, mut app, _) = crate::input_api::tests::setup();
    let wire = Arc::new(Wire::default());
    let manager = InputManager::new(app.active.clone(), Arc::new(BackendWire(wire.clone())));
    manager.set_logger(&app.logs);
    Arc::get_mut(&mut app).unwrap().input = manager;
    (dir, app, wire)
}
#[tokio::test]
async fn queued_input_cannot_cross_reconnection_even_if_wire_session_id_is_reused() {
    let (_dir, app, wire) = setup();
    let b = app.active.bind().unwrap();
    let owner = owner();
    let held = app.input.keyboard_gate.lock().await;
    let request = tokio::spawn({
        let manager = app.input.clone();
        let b = b.clone();
        let owner = owner.clone();
        async move {
            manager
                .apply(
                    owner,
                    b,
                    Action::Key {
                        down: true,
                        usage: 4,
                    },
                )
                .await
        }
    });
    tokio::time::timeout(Duration::from_secs(1), async {
        while owner.pin.get().is_none() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    wire.connection.store(2, Ordering::Release);
    drop(held);
    assert_eq!(request.await.unwrap().err().unwrap().code, "STALE_SESSION");
    assert!(owner.cancelled.is_cancelled());
    assert!(wire.calls.lock().is_empty());
    app.shutdown().await;
}
#[tokio::test]
async fn backend_rechecks_original_epoch_immediately_before_emitting_a_packet() {
    let (_dir, app, wire) = setup();
    let b = app.active.bind().unwrap();
    let owner = owner();
    wire.pause.store(true, Ordering::Release);
    let request = tokio::spawn({
        let manager = app.input.clone();
        async move {
            manager
                .apply(
                    owner,
                    b,
                    Action::Key {
                        down: true,
                        usage: 4,
                    },
                )
                .await
        }
    });
    wire.entered.notified().await;
    wire.connection.store(2, Ordering::Release);
    wire.resume.notify_one();
    assert_eq!(request.await.unwrap().err().unwrap().code, "STALE_SESSION");
    assert!(wire.calls.lock().is_empty());
    app.shutdown().await;
}
#[tokio::test]
async fn automatic_release_stays_on_original_epoch_and_explicit_recovery_can_reset_current_epoch() {
    let (_dir, app, wire) = setup();
    let b = app.active.bind().unwrap();
    let a = owner();
    app.input
        .apply(
            a.clone(),
            b.clone(),
            Action::Key {
                down: true,
                usage: 4,
            },
        )
        .await
        .unwrap();
    wire.connection.store(2, Ordering::Release);
    assert_eq!(
        app.input
            .release_owner(a.id.clone())
            .await
            .err()
            .unwrap()
            .code,
        "HID_RELEASE_UNCONFIRMED"
    );
    assert_eq!(wire.calls.lock().len(), 1);
    assert!(app.input.snapshot(&b).release_unconfirmed);
    assert!(app.input.snapshot(&b).ready);
    app.input.recover(b.clone()).await.unwrap();
    assert!(!app.input.snapshot(&b).release_unconfirmed);
    let calls = wire.calls.lock().clone();
    assert_eq!(calls.len(), 2);
    assert_eq!(
        calls[0].epoch,
        Epoch {
            connection: 1,
            session: 7
        }
    );
    assert_eq!(
        calls[1].epoch,
        Epoch {
            connection: 2,
            session: 7
        }
    );
    assert_eq!(calls[1].op, 0x107);
    assert!(calls[1].cleanup);
    assert_eq!(
        app.input
            .apply(
                a,
                b,
                Action::Key {
                    down: false,
                    usage: 4
                }
            )
            .await
            .err()
            .unwrap()
            .code,
        "STALE_SESSION"
    );
    assert_eq!(wire.calls.lock().len(), 2);
    app.shutdown().await;
}
#[tokio::test]
async fn ui_fifo_pins_at_enqueue_and_does_not_resolve_a_new_device_when_drained() {
    let (_dir, app, wire) = setup();
    let b = app.active.bind().unwrap();
    let a = owner();
    let errors = Arc::new(Mutex::new(None));
    let broker =
        crate::ui_input::UiInput::new(&app, &tokio::runtime::Handle::current(), errors.clone());
    broker
        .action(
            a.clone(),
            b,
            Action::Key {
                down: true,
                usage: 4,
            },
        )
        .unwrap();
    assert!(a.pin.get().is_some());
    wire.connection.store(2, Ordering::Release);
    tokio::time::timeout(Duration::from_secs(1), async {
        while !a.cancelled.is_cancelled() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(wire.calls.lock().is_empty());
    assert!(a.cancelled.is_cancelled());
    assert!(errors.lock().is_none()); // stale input is diagnostic-only, never retargeted
    broker.stop();
    app.shutdown().await;
}
#[tokio::test]
async fn input_open_and_heartbeat_use_the_same_management_epoch_without_hid_effects() {
    let (_dir, app, wire) = setup();
    let principal = Principal {
        key_id: "mock".into(),
        client_id: "mock".into(),
        cancelled: CancellationToken::new(),
        ended: CancellationToken::new(),
    };
    let opened = app
        .execute_remote(&principal, "input_open", serde_json::json!({}))
        .await
        .unwrap();
    assert!(wire.calls.lock().is_empty());
    wire.connection.store(2, Ordering::Release);
    let args = serde_json::json!({"input_session":opened["input_session"]});
    assert_eq!(
        app.execute_remote(&principal, "input_heartbeat", args.clone())
            .await
            .err()
            .unwrap()
            .code,
        "STALE_SESSION"
    );
    assert!(wire.calls.lock().is_empty());
    app.execute_remote(&principal, "input_close", args)
        .await
        .unwrap();
    assert!(wire.calls.lock().is_empty());
    app.shutdown().await;
}

#[tokio::test]
async fn explicit_recovery_pins_before_waiting_for_the_input_gate() {
    let (_dir, app, wire) = setup();
    let b = app.active.bind().unwrap();
    let held = app.input.cleanup_gate.lock().await;
    let request = tokio::spawn({
        let manager = app.input.clone();
        async move { manager.recover(b).await }
    });
    tokio::time::timeout(Duration::from_secs(1), wire.sampled.notified())
        .await
        .unwrap();
    wire.connection.store(2, Ordering::Release);
    drop(held);
    assert_eq!(
        request.await.unwrap().err().unwrap().code,
        "HID_RELEASE_UNCONFIRMED"
    );
    assert!(wire.calls.lock().is_empty());
    app.shutdown().await;
}

#[cfg(windows)]
#[tokio::test]
async fn delayed_native_key_uses_the_epoch_pinned_before_capture_was_armed() {
    let (_dir, app, wire) = setup();
    let b = app.active.bind().unwrap();
    let a = owner();
    app.input.prepare_owner(&a, &b).unwrap();
    let gate = crate::keyboard_capture::windows::Gate {
        epoch: 42,
        binding: b,
        owner: a.clone(),
    };
    let broker = crate::ui_input::UiInput::new(
        &app,
        &tokio::runtime::Handle::current(),
        Arc::new(Mutex::new(None)),
    );
    // Same connection identity, changed HELLO wire session.
    wire.session.store(8, Ordering::Release);
    assert_eq!(
        gate.dispatch(
            &app.active,
            &broker,
            crate::keyboard_capture::Event::Key {
                epoch: 42,
                usage: 4,
                down: true,
            }
        )
        .err()
        .unwrap()
        .code,
        "STALE_SESSION"
    );
    assert!(a.cancelled.is_cancelled());
    assert!(wire.calls.lock().is_empty());
    broker.stop();
    app.shutdown().await;
}
