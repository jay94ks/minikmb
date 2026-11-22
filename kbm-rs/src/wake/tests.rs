use super::*;
use parking_lot::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio_util::sync::CancellationToken;
type Hook = Option<Box<dyn Fn() + Send>>;
#[derive(Default)]
struct Fake {
    listed: AtomicUsize,
    packets: Mutex<Vec<Vec<u8>>>,
    hook: Mutex<Hook>,
    unsupported: bool,
    missing: bool,
}
impl Backend for Fake {
    fn adapters(&self) -> Result<Vec<Adapter>, AppError> {
        self.listed.fetch_add(1, Ordering::Relaxed);
        Ok(if self.missing {
            vec![]
        } else {
            vec![Adapter {
                name: "owned fake".into(),
                address: "192.168.1.4".into(),
                broadcast: "192.168.1.255".into(),
            }]
        })
    }
    fn send(
        &self,
        a: &Adapter,
        p: &[u8; 102],
        guard: &dyn Fn() -> Result<(), AppError>,
    ) -> Result<Code, AppError> {
        if let Some(hook) = self.hook.lock().take() {
            hook();
        }
        guard()?;
        assert_eq!(a.address, "192.168.1.4");
        assert_eq!(a.broadcast, "192.168.1.255");
        if self.unsupported {
            return Ok(Code::IgnoredUnsupported);
        }
        self.packets.lock().push(p.to_vec());
        Ok(Code::SentUnconfirmed)
    }
}
fn owner() -> Principal {
    Principal {
        key_id: "mock".into(),
        client_id: "mock".into(),
        cancelled: CancellationToken::new(),
        ended: CancellationToken::new(),
    }
}
fn setup(fake: Arc<Fake>) -> (tempfile::TempDir, Arc<AppService>) {
    let (dir, mut app, _wire) = crate::input_api::tests::setup();
    Arc::get_mut(&mut app).unwrap().wake = Service::new(fake);
    (dir, app)
}
fn config() -> Value {
    json!({"enabled":true,"mac":" 02-AA-BB-CC-DD-EE ","local_address":"192.168.1.4"})
}
#[test]
fn magic_packet_and_directed_broadcast_match_v1_rules() {
    for mac in ["02aabbccddee", "02:aa:bb:cc:dd:ee", " 02-AA-BB-CC-DD-EE "] {
        assert_eq!(normalize_mac(mac).unwrap(), "02:aa:bb:cc:dd:ee");
        let packet = magic_packet(mac).unwrap();
        assert_eq!(&packet[..6], &[255; 6]);
        for bytes in packet[6..].chunks_exact(6) {
            assert_eq!(bytes, [2, 170, 187, 204, 221, 238]);
        }
    }
    for mac in [
        "01:aa:bb:cc:dd:ee",
        "ff:ff:ff:ff:ff:ff",
        "00:00:00:00:00:00",
        "02-aa:bb-cc:dd-ee",
        "02aabbccddee00",
        "hello",
    ] {
        assert!(magic_packet(mac).is_err());
    }
    assert_eq!(
        directed_broadcast("192.168.2.7".parse().unwrap(), 23),
        Some("192.168.3.255".parse().unwrap())
    );
    assert_eq!(
        directed_broadcast("10.4.7.6".parse().unwrap(), 16),
        Some("10.4.255.255".parse().unwrap())
    );
    for prefix in [0, 31, 32, 255] {
        assert!(directed_broadcast("192.168.1.4".parse().unwrap(), prefix).is_none());
    }
    assert!(directed_broadcast(Ipv4Addr::LOCALHOST, 24).is_none());
}
#[tokio::test]
async fn configure_never_sends_and_explicit_send_is_unconfirmed_uid_scoped_and_not_retried() {
    let fake = Arc::new(Fake::default());
    let (_dir, app) = setup(fake.clone());
    let user = owner();
    let b = app.active.bind().unwrap();
    let ignored = app
        .execute_remote(&user, "wake_send", json!({}))
        .await
        .unwrap();
    assert_eq!(ignored["code"], 129);
    assert_eq!(fake.listed.load(Ordering::Relaxed), 0);
    let saved = app
        .execute_remote(&user, "wake_configure", config())
        .await
        .unwrap();
    assert_eq!(saved["config"]["mac"], "02:aa:bb:cc:dd:ee");
    assert!(fake.packets.lock().is_empty());
    let sent = app
        .execute_remote(&user, "wake_send", json!({}))
        .await
        .unwrap();
    assert_eq!(sent["code"], 128);
    assert_eq!(sent["wake_confirmed"], false);
    assert_eq!(fake.packets.lock().len(), 1);
    app.active.begin().unwrap();
    app.active
        .finish(Some(crate::model::Uid::from_bytes(&[2; 12]).unwrap()));
    let status = app
        .execute_remote(&user, "wake_get_status", json!({}))
        .await
        .unwrap();
    assert_eq!(status["config"]["enabled"], false);
    assert_eq!(status["usb_wake_supported"], false);
    app.execute_remote(&user, "wake_send", json!({}))
        .await
        .unwrap();
    assert_eq!(fake.packets.lock().len(), 1);
    assert!(app.preferences.channel(&b.uid).wol.unwrap().enabled);
    app.shutdown().await;
}
#[tokio::test]
async fn missing_adapter_unsupported_and_changed_owner_never_report_wake_success() {
    for missing in [false, true] {
        let fake = Arc::new(Fake {
            unsupported: true,
            missing,
            ..Default::default()
        });
        let (_dir, app) = setup(fake.clone());
        let user = owner();
        app.execute_remote(&user, "wake_configure", config())
            .await
            .unwrap();
        let result = app.execute_remote(&user, "wake_send", json!({})).await;
        if missing {
            assert_eq!(result.unwrap_err().code, "WOL_ADAPTER_MISSING");
        } else {
            assert_eq!(result.unwrap()["code"], 130);
        }
        assert!(fake.packets.lock().is_empty());
        app.shutdown().await;
    }
    for revoke in [false, true] {
        let fake = Arc::new(Fake::default());
        let (_dir, app) = setup(fake.clone());
        let user = owner();
        app.execute_remote(&user, "wake_configure", config())
            .await
            .unwrap();
        let active = app.active.clone();
        let b = active.bind().unwrap();
        let token = user.cancelled.clone();
        *fake.hook.lock() = Some(Box::new(move || {
            if revoke {
                token.cancel();
            } else {
                active.disconnected(&b.uid);
            }
        }));
        let e = app
            .execute_remote(&user, "wake_send", json!({}))
            .await
            .unwrap_err();
        assert_eq!(
            e.code,
            if revoke {
                "UNAUTHORIZED"
            } else {
                "STALE_OPERATION"
            }
        );
        assert!(fake.packets.lock().is_empty());
        app.shutdown().await;
    }
}
#[tokio::test]
async fn api_rejects_uid_overrides_queue_overflow_and_calls_without_active() {
    let fake = Arc::new(Fake::default());
    let (_dir, app) = setup(fake.clone());
    let user = owner();
    for args in [
        json!({}),
        json!({"enabled":true}),
        json!({"enabled":"true","mac":"","local_address":""}),
    ] {
        assert_eq!(
            app.execute_remote(&user, "wake_configure", args)
                .await
                .unwrap_err()
                .code,
            "INVALID_ARGUMENTS"
        );
    }
    for args in [
        json!({"uid":"other"}),
        json!({"port":9}),
        json!({"target_ip":"192.168.1.255"}),
    ] {
        assert_eq!(
            app.execute_remote(&user, "wake_send", args)
                .await
                .unwrap_err()
                .code,
            "INVALID_ARGUMENTS"
        );
    }
    let held = app.wake.slots.clone().acquire_many_owned(2).await.unwrap();
    assert_eq!(
        app.execute_remote(&user, "wake_send", json!({}))
            .await
            .unwrap_err()
            .code,
        "QUEUE_FULL"
    );
    drop(held);
    app.active.begin().unwrap();
    app.active.finish(None);
    assert_eq!(
        app.execute_remote(&user, "wake_send", json!({}))
            .await
            .unwrap_err()
            .code,
        "NO_ACTIVE_CHANNEL"
    );
    assert!(fake.packets.lock().is_empty());
    assert_eq!(fake.listed.load(Ordering::Relaxed), 0);
    app.shutdown().await;
}
