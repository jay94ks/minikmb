use super::*;
use crate::{network::wire, protocol::Frame, service::AppService};
use futures_util::future::BoxFuture;
use std::sync::atomic::AtomicU64;
use tokio::sync::Notify;
#[derive(Clone)]
struct Call {
    uid: Uid,
    session: u64,
    op: u16,
    p: Vec<u8>,
    cleanup: bool,
}
#[derive(Default)]
struct Device {
    enabled: bool,
    sent: u64,
    limit: u64,
    sequence: u32,
    rx_id: u32,
    last_tx: u32,
    partial: Vec<u8>,
    partial_id: u32,
    total: usize,
    crc: u32,
    frames: Vec<Vec<u8>>,
}
pub(crate) struct Fake {
    devices: Mutex<HashMap<Uid, Device>>,
    calls: Mutex<Vec<Call>>,
    events: broadcast::Sender<DeviceEvent>,
    session: AtomicU64,
    hold_on: AtomicBool,
    hold_renew: AtomicBool,
    hold_prepare: AtomicBool,
    drain_old: AtomicBool,
    deny_off: AtomicBool,
    bad_ack: AtomicBool,
    hold_frame: AtomicBool,
    frame_waiting: AtomicBool,
    host: Mutex<Option<crate::network::host_io::Host>>,
    resume: Notify,
}
impl Default for Fake {
    fn default() -> Self {
        Self {
            devices: Default::default(),
            calls: Default::default(),
            events: broadcast::channel(2048).0,
            session: AtomicU64::new(7),
            hold_on: AtomicBool::new(false),
            hold_renew: AtomicBool::new(false),
            hold_prepare: AtomicBool::new(false),
            drain_old: AtomicBool::new(false),
            deny_off: AtomicBool::new(false),
            bad_ack: AtomicBool::new(false),
            hold_frame: AtomicBool::new(false),
            frame_waiting: AtomicBool::new(false),
            host: Default::default(),
            resume: Default::default(),
        }
    }
}
struct FakeBackend {
    fake: Arc<Fake>,
    active: Arc<ActiveCoordinator>,
}
struct FakeWire {
    fake: Arc<Fake>,
    active: Arc<ActiveCoordinator>,
    uid: Uid,
    session: u64,
}
impl Backend for FakeBackend {
    fn host(&self) -> BoxFuture<'static, Result<crate::network::host_io::Host, AppError>> {
        let host = self.fake.host.lock().clone();
        Box::pin(async move {
            host.ok_or_else(|| AppError::new("DNS_UNAVAILABLE", "모의 DNS 조회 실패"))
        })
    }
    fn connect(&self, b: &Binding) -> Result<Connection, AppError> {
        Ok(Connection {
            session: self.fake.session.load(Ordering::Acquire),
            wire: Arc::new(FakeWire {
                fake: self.fake.clone(),
                active: self.active.clone(),
                uid: b.uid.clone(),
                session: self.fake.session.load(Ordering::Acquire),
            }),
            events: self.fake.events.subscribe(),
        })
    }
    fn prepare_target_change(&self, b: Binding) -> BoxFuture<'static, Result<(), AppError>> {
        let fake = self.fake.clone();
        Box::pin(async move {
            fake.calls.lock().push(Call {
                uid: b.uid,
                session: fake.session.load(Ordering::Acquire),
                op: 0xffff,
                p: vec![],
                cleanup: false,
            });
            if fake.hold_prepare.load(Ordering::Acquire) {
                fake.resume.notified().await;
            }
            Ok(())
        })
    }
}
fn u32at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}
impl Fake {
    fn command(
        &self,
        uid: Uid,
        session: u64,
        ch: u8,
        op: u16,
        p: Vec<u8>,
        cleanup: bool,
    ) -> Result<Vec<u8>, AppError> {
        assert!(wire::allowed(ch, op, &p));
        if cleanup {
            assert_eq!(ch, 4);
            assert!((op == 0x401 && p == [0]) || (op == 0x403 && p.is_empty()));
        }
        self.calls.lock().push(Call {
            uid: uid.clone(),
            session,
            op,
            p: p.clone(),
            cleanup,
        });
        let mut devices = self.devices.lock();
        let d = devices.entry(uid.clone()).or_default();
        match op {
            0x401 => {
                if p == [0] && self.deny_off.load(Ordering::Acquire) {
                    return Err(AppError::mcu(3));
                }
                d.enabled = p == [1];
                d.partial.clear();
                Ok(vec![])
            }
            0x403 => {
                let mut b = vec![0; 16];
                b[12] = u8::from(d.enabled);
                b[13] = u8::from(d.enabled);
                Ok(b)
            }
            6 => {
                let limit = u64::from_le_bytes(p[4..12].try_into().unwrap());
                if limit == 0 && !d.enabled && self.drain_old.swap(false, Ordering::AcqRel) {
                    d.rx_id += 1;
                    for payload in framing::fragments(d.rx_id, &discover()).unwrap() {
                        d.sequence += 1;
                        d.sent += payload.len() as u64;
                        assert!(d.sent <= d.limit);
                        let _ = self.events.send(DeviceEvent {
                            uid: uid.clone(),
                            session,
                            frame: Frame {
                                session,
                                kind: 4,
                                channel: 4,
                                opcode: 0x480,
                                sequence: d.sequence,
                                payload,
                            },
                        });
                    }
                }
                if limit > d.limit {
                    assert!(limit >= d.sent && limit - d.sent <= framing::WINDOW);
                    d.limit = limit;
                }
                Ok([d.sent.to_le_bytes(), d.limit.to_le_bytes()].concat())
            }
            0x402 => {
                assert!(d.enabled);
                let id = u32at(&p, 0);
                let offset = u16::from_le_bytes(p[4..6].try_into().unwrap()) as usize;
                let total = u16::from_le_bytes(p[6..8].try_into().unwrap()) as usize;
                if offset == 0 {
                    assert!(id > d.last_tx);
                    assert!(d.partial.is_empty());
                    d.last_tx = id;
                    d.partial_id = id;
                    d.total = total;
                    d.crc = u32at(&p, 8);
                }
                assert_eq!(id, d.partial_id);
                assert_eq!(offset, d.partial.len());
                assert_eq!(p.len() - 16, (total - offset).min(1008));
                assert_eq!(u32at(&p, 8), d.crc);
                d.partial.extend_from_slice(&p[16..]);
                if d.partial.len() == d.total {
                    assert_eq!(crc32fast::hash(&d.partial), d.crc);
                    let frame = std::mem::take(&mut d.partial);
                    d.frames.push(frame);
                }
                Ok(if self.bad_ack.load(Ordering::Acquire) {
                    vec![1]
                } else {
                    vec![]
                })
            }
            _ => panic!("unexpected op {op:x}"),
        }
    }
    fn deliver(&self, uid: &Uid, bytes: &[u8]) {
        let mut devices = self.devices.lock();
        let d = devices.get_mut(uid).unwrap();
        assert!(d.enabled);
        d.rx_id += 1;
        for p in framing::fragments(d.rx_id, bytes).unwrap() {
            d.sequence += 1;
            d.sent += p.len() as u64;
            assert!(d.sent <= d.limit);
            let session = self.session.load(Ordering::Acquire);
            let _ = self.events.send(DeviceEvent {
                uid: uid.clone(),
                session,
                frame: Frame {
                    session,
                    kind: 4,
                    channel: 4,
                    opcode: 0x480,
                    sequence: d.sequence,
                    payload: p,
                },
            });
        }
    }
    fn reconnect(&self) {
        self.session.fetch_add(1, Ordering::AcqRel);
        self.devices.lock().clear();
    }
}
impl Wire for FakeWire {
    fn live(&self) -> bool {
        self.fake.session.load(Ordering::Acquire) == self.session
    }
    fn send(
        &self,
        b: Binding,
        owner: Principal,
        life: CancellationToken,
        ch: u8,
        op: u16,
        p: Vec<u8>,
    ) -> BoxFuture<'static, Result<Vec<u8>, AppError>> {
        let fake = self.fake.clone();
        let active = self.active.clone();
        let uid = self.uid.clone();
        let session = self.session;
        Box::pin(async move {
            active.validate(&b)?;
            owner_check(&owner)?;
            if life.is_cancelled() {
                return Err(AppError::new("OPERATION_CANCELLED", "mock cancelled"));
            }
            if fake.session.load(Ordering::Acquire) != session {
                return Err(cleanup_error());
            }
            if op == 0x402 && fake.hold_frame.load(Ordering::Acquire) {
                fake.frame_waiting.store(true, Ordering::Release);
                fake.resume.notified().await;
                active.validate(&b)?;
                owner_check(&owner)?;
                if life.is_cancelled() {
                    return Err(AppError::new(
                        "OPERATION_CANCELLED",
                        "queued frame cancelled",
                    ));
                }
            }
            let hold = (op == 0x401 && p == [1] && fake.hold_on.load(Ordering::Acquire))
                || (op == 6
                    && fake.devices.lock().get(&uid).is_some_and(|d| d.enabled)
                    && fake.hold_renew.load(Ordering::Acquire));
            let reply = fake.command(uid, session, ch, op, p, false);
            if hold {
                fake.resume.notified().await;
            }
            reply
        })
    }
    fn cleanup(&self, op: u16) -> BoxFuture<'static, Result<Vec<u8>, AppError>> {
        let fake = self.fake.clone();
        let uid = self.uid.clone();
        let session = self.session;
        Box::pin(async move {
            if fake.session.load(Ordering::Acquire) != session {
                return Err(cleanup_error());
            }
            fake.command(
                uid,
                session,
                4,
                op,
                if op == 0x401 { vec![0] } else { vec![] },
                true,
            )
        })
    }
}
pub(crate) fn install(app: &mut Arc<AppService>) -> Arc<Fake> {
    let fake = Arc::new(Fake::default());
    let manager = Manager::with_preferences(
        app.active.clone(),
        Arc::new(FakeBackend {
            fake: fake.clone(),
            active: app.active.clone(),
        }),
        app.preferences.clone(),
    );
    Arc::get_mut(app).unwrap().network = manager;
    fake
}
fn setup() -> (tempfile::TempDir, Arc<AppService>, Arc<Fake>, Principal) {
    let (dir, mut app, _) = crate::input_api::tests::setup();
    let fake = install(&mut app);
    let owner = Principal {
        key_id: "test".into(),
        client_id: "test".into(),
        cancelled: CancellationToken::new(),
        ended: CancellationToken::new(),
    };
    (dir, app, fake, owner)
}
async fn until<F: Fn() -> bool>(f: F) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !f() {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
}
fn discover() -> Vec<u8> {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/v1.json")).unwrap();
    fixture["dhcp"]["request"]
        .as_str()
        .unwrap()
        .as_bytes()
        .chunks_exact(2)
        .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
        .collect()
}
async fn mode(app: &Arc<AppService>, p: &Principal, m: Mode) -> Result<Status, AppError> {
    app.network
        .set_mode(app.active.bind().unwrap(), p.clone(), m)
        .await
}
#[tokio::test]
async fn off_on_keeps_ids_credit_and_complete_resource_survives_owner_close() {
    let (_dir, app, fake, p) = setup();
    let uid = app.active.bind().unwrap().uid;
    assert_eq!(
        app.network.status(&app.active.bind().unwrap()).mode,
        Mode::Off
    );
    mode(&app, &p, Mode::Emulation).await.unwrap();
    {
        let calls = fake.calls.lock();
        let on = calls
            .iter()
            .position(|c| c.op == 0x401 && c.p == [1])
            .unwrap();
        assert!(calls[..on].iter().any(|c| c.op == 0xffff));
        assert!(calls[..on].iter().any(|c| c.op == 0x401 && c.p == [0]));
    }
    fake.deliver(&uid, &discover());
    until(|| fake.devices.lock()[&uid].frames.len() == 1).await;
    mode(&app, &p, Mode::Off).await.unwrap();
    let sent = fake.devices.lock()[&uid].sent;
    assert!(sent > 0);
    let on = mode(&app, &p, Mode::Emulation).await.unwrap();
    assert!(on.mcu.enabled);
    let before = fake.calls.lock().len();
    mode(&app, &p, Mode::Emulation).await.unwrap();
    assert_eq!(fake.calls.lock().len(), before);
    p.ended.cancel();
    fake.deliver(&uid, &discover());
    until(|| fake.devices.lock()[&uid].frames.len() == 2).await;
    assert_eq!(fake.devices.lock()[&uid].last_tx, 2);
    assert!(fake.devices.lock()[&uid].limit >= sent + framing::WINDOW);
    app.select(None).await.unwrap();
    assert!(!fake.devices.lock()[&uid].enabled);
    assert!(app.network.resource().is_none());
}
#[tokio::test]
async fn receive_before_enable_ack_and_credit_ack_is_admitted_but_never_replayed() {
    let (_dir, app, fake, p) = setup();
    let uid = app.active.bind().unwrap().uid;
    fake.hold_on.store(true, Ordering::Release);
    let a = app.clone();
    let owner = p.clone();
    let on = tokio::spawn(async move { mode(&a, &owner, Mode::Emulation).await });
    until(|| fake.devices.lock().get(&uid).is_some_and(|d| d.enabled)).await;
    fake.deliver(&uid, &discover());
    fake.hold_on.store(false, Ordering::Release);
    fake.resume.notify_one();
    on.await.unwrap().unwrap();
    until(|| fake.devices.lock()[&uid].frames.len() == 1).await;
    fake.hold_renew.store(true, Ordering::Release);
    for _ in 0..55 {
        fake.deliver(&uid, &discover());
    }
    until(|| fake.devices.lock()[&uid].limit > framing::WINDOW).await;
    for _ in 0..40 {
        fake.deliver(&uid, &discover());
    }
    fake.hold_renew.store(false, Ordering::Release);
    fake.resume.notify_one();
    until(|| app.network.status(&app.active.bind().unwrap()).rx_frames == 96).await;
    let s = app.network.status(&app.active.bind().unwrap());
    assert!(s.error.is_none());
    assert!(s.tx_queued <= 64);
    app.network.shutdown().await;
}
#[tokio::test]
async fn revoked_key_during_acceptance_cleans_original_without_exposing_enabled_resource() {
    let (_dir, app, fake, p) = setup();
    let uid = app.active.bind().unwrap().uid;
    fake.hold_on.store(true, Ordering::Release);
    let a = app.clone();
    let owner = p.clone();
    let job = tokio::spawn(async move { mode(&a, &owner, Mode::Emulation).await });
    until(|| fake.devices.lock().get(&uid).is_some_and(|d| d.enabled)).await;
    p.cancelled.cancel();
    fake.hold_on.store(false, Ordering::Release);
    fake.resume.notify_one();
    assert_eq!(job.await.unwrap().err().unwrap().code, "UNAUTHORIZED");
    assert!(!fake.devices.lock()[&uid].enabled);
    assert!(!app.network.resource().unwrap().cleanup_required);
    assert!(fake
        .calls
        .lock()
        .iter()
        .any(|c| c.cleanup && c.op == 0x401 && c.session == 7 && c.uid == uid));
}
#[tokio::test]
async fn accepted_mode_change_finishes_after_requester_task_is_dropped() {
    let (_dir, app, fake, p) = setup();
    let uid = app.active.bind().unwrap().uid;
    fake.hold_on.store(true, Ordering::Release);
    let a = app.clone();
    let owner = p.clone();
    let job = tokio::spawn(async move { mode(&a, &owner, Mode::Emulation).await });
    until(|| fake.devices.lock().get(&uid).is_some_and(|d| d.enabled)).await;
    job.abort();
    fake.hold_on.store(false, Ordering::Release);
    fake.resume.notify_one();
    until(|| app.network.resource().is_some_and(|s| s.phase == "running")).await;
    app.network.shutdown().await;
    assert!(!fake.devices.lock()[&uid].enabled);
}
#[tokio::test]
async fn cleanup_failure_blocks_other_uid_until_original_off_is_verified() {
    let (_dir, app, fake, p) = setup();
    let b = app.active.bind().unwrap();
    mode(&app, &p, Mode::Emulation).await.unwrap();
    fake.deny_off.store(true, Ordering::Release);
    assert!(app
        .select(Some(Uid::from_bytes(&[2; 12]).unwrap()))
        .await
        .is_err());
    assert_eq!(app.active.bind().unwrap().uid, b.uid);
    assert!(app.network.resource().unwrap().cleanup_required);
    fake.deny_off.store(false, Ordering::Release);
    mode(&app, &p, Mode::Off).await.unwrap();
    app.select(Some(Uid::from_bytes(&[2; 12]).unwrap()))
        .await
        .unwrap();
    assert!(!fake.devices.lock()[&b.uid].enabled);
    assert!(fake
        .calls
        .lock()
        .iter()
        .filter(|c| c.cleanup)
        .all(|c| c.uid == b.uid));
}
#[tokio::test]
async fn dead_session_requires_original_uid_recovery_and_never_retargets_old_frames() {
    let (_dir, app, fake, p) = setup();
    let uid = app.active.bind().unwrap().uid;
    mode(&app, &p, Mode::Emulation).await.unwrap();
    fake.reconnect();
    until(|| {
        app.network
            .resource()
            .is_some_and(|s| s.cleanup_error.is_some())
    })
    .await;
    assert!(app
        .select(Some(Uid::from_bytes(&[2; 12]).unwrap()))
        .await
        .is_err());
    mode(&app, &p, Mode::Off).await.unwrap();
    assert!(fake
        .calls
        .lock()
        .iter()
        .filter(|c| c.session == 8)
        .all(|c| c.uid == uid && c.op != 0x402));
    mode(&app, &p, Mode::Emulation).await.unwrap();
    fake.deliver(&uid, &discover());
    until(|| fake.devices.lock()[&uid].last_tx == 1).await;
    app.network.shutdown().await;
}
#[tokio::test]
async fn malformed_data_ack_is_not_retried_and_restart_spends_a_new_frame_id() {
    let (_dir, app, fake, p) = setup();
    let uid = app.active.bind().unwrap().uid;
    mode(&app, &p, Mode::Emulation).await.unwrap();
    fake.bad_ack.store(true, Ordering::Release);
    fake.deliver(&uid, &discover());
    until(|| {
        app.network
            .resource()
            .is_some_and(|s| s.phase == "failed" && !s.cleanup_required)
    })
    .await;
    assert_eq!(
        fake.calls.lock().iter().filter(|c| c.op == 0x402).count(),
        1
    );
    fake.bad_ack.store(false, Ordering::Release);
    mode(&app, &p, Mode::Emulation).await.unwrap();
    fake.deliver(&uid, &discover());
    until(|| fake.devices.lock()[&uid].last_tx == 2).await;
    app.network.shutdown().await;
}
#[tokio::test]
async fn configuration_and_unsupported_host_mode_never_modify_live_network() {
    let (_dir, app, fake, p) = setup();
    let b = app.active.bind().unwrap();
    let c = Config {
        server_ip: "10.20.30.1".into(),
        ..Default::default()
    };
    app.network.configure(&b, &p, c.clone()).unwrap();
    assert_eq!(app.network.status(&b).config, c);
    mode(&app, &p, Mode::Emulation).await.unwrap();
    let n = fake.calls.lock().len();
    assert_eq!(
        mode(&app, &p, Mode::Host).await.err().unwrap().code,
        "DNS_UNAVAILABLE"
    );
    assert_eq!(fake.calls.lock().len(), n);
    assert!(app.network.configure(&b, &p, Config::default()).is_err());
    app.shutdown().await;
    assert!(!fake.devices.lock()[&b.uid].enabled);
}

#[tokio::test]
async fn saved_network_and_rules_reload_without_opening_any_device_or_listener() {
    let (dir, app, fake, p) = setup();
    let b = app.active.bind().unwrap();
    let config = Config {
        server_ip: "10.20.30.1".into(),
        ..Default::default()
    };
    app.network.configure(&b, &p, config.clone()).unwrap();
    let rule = forwarding::Rule {
        id: "saved".into(),
        protocol: forwarding::Protocol::Tcp,
        listen_port: 18022,
        target_ip: "10.20.30.100".into(),
        target_port: 22,
    };
    app.network
        .configure_forward(&b, &p, vec![rule.clone()])
        .unwrap();
    assert!(fake.calls.lock().is_empty());
    let mut reopened = AppService::new(dir.path().to_owned()).unwrap();
    let second = install(&mut reopened);
    let status = reopened.network.status(&b);
    assert_eq!(status.mode, Mode::Off);
    assert_eq!(status.config, config);
    assert_eq!(
        serde_json::to_value(&status.forwarding.rules).unwrap(),
        serde_json::to_value(vec![rule]).unwrap()
    );
    assert!(!status.forwarding.running);
    assert!(second.calls.lock().is_empty());
    assert!(reopened.network.resource().is_none());
    assert!(reopened.active.bind().is_err());
    app.shutdown().await;
    reopened.shutdown().await;
}

#[tokio::test]
async fn timed_out_stop_keeps_worker_handle_and_waits_for_its_last_off_before_new_on() {
    let (_dir, app, fake, p) = setup();
    let uid = app.active.bind().unwrap().uid;
    mode(&app, &p, Mode::Emulation).await.unwrap();
    fake.hold_prepare.store(true, Ordering::Release);
    assert_eq!(
        mode(&app, &p, Mode::Off).await.err().unwrap().code,
        "NETWORK_CLEANUP_REQUIRED"
    );
    assert!(app
        .network
        .state
        .lock()
        .current
        .as_ref()
        .unwrap()
        .worker
        .lock()
        .is_some());
    assert!(fake.devices.lock()[&uid].enabled);
    let a = app.clone();
    let owner = p.clone();
    let restart = tokio::spawn(async move { mode(&a, &owner, Mode::Emulation).await });
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert!(!restart.is_finished());
    fake.hold_prepare.store(false, Ordering::Release);
    fake.resume.notify_one();
    restart.await.unwrap().unwrap();
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert!(fake.devices.lock()[&uid].enabled);
    app.network.shutdown().await;
}

#[tokio::test]
async fn queued_control_requests_are_bounded_and_canceled_owners_never_enable() {
    let (_dir, app, fake, p) = setup();
    let uid = app.active.bind().unwrap().uid;
    fake.hold_on.store(true, Ordering::Release);
    let mut jobs = vec![];
    for _ in 0..4 {
        let a = app.clone();
        let owner = p.clone();
        jobs.push(tokio::spawn(async move {
            mode(&a, &owner, Mode::Emulation).await
        }));
    }
    until(|| {
        app.network.controls.available_permits() == 0
            && fake.devices.lock().get(&uid).is_some_and(|d| d.enabled)
    })
    .await;
    assert_eq!(
        mode(&app, &p, Mode::Emulation).await.err().unwrap().code,
        "QUEUE_FULL"
    );
    p.ended.cancel();
    fake.hold_on.store(false, Ordering::Release);
    fake.resume.notify_one();
    for job in jobs {
        assert!(job.await.unwrap().is_err());
    }
    assert!(!fake.devices.lock()[&uid].enabled);
    assert_eq!(
        fake.calls
            .lock()
            .iter()
            .filter(|c| c.op == 0x401 && c.p == [1])
            .count(),
        1
    );
}

#[tokio::test]
async fn old_stream_before_credit_query_ack_is_drained_without_reinjecting_a_packet() {
    let (_dir, app, fake, p) = setup();
    let uid = app.active.bind().unwrap().uid;
    mode(&app, &p, Mode::Emulation).await.unwrap();
    fake.deliver(&uid, &discover());
    until(|| fake.devices.lock()[&uid].frames.len() == 1).await;
    mode(&app, &p, Mode::Off).await.unwrap();
    fake.drain_old.store(true, Ordering::Release);
    mode(&app, &p, Mode::Emulation).await.unwrap();
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(app.network.status(&app.active.bind().unwrap()).rx_frames, 0);
    assert_eq!(fake.devices.lock()[&uid].frames.len(), 1);
    fake.deliver(&uid, &discover());
    until(|| fake.devices.lock()[&uid].frames.len() == 2).await;
    assert_eq!(app.network.status(&app.active.bind().unwrap()).rx_frames, 1);
    app.network.shutdown().await;
}

#[tokio::test]
async fn host_mode_advertises_dns_and_is_exclusive_with_emulation() {
    let (_dir, app, fake, p) = setup();
    *fake.host.lock() = Some(crate::network::host_io::Host {
        dns: "127.0.0.1:53".parse().unwrap(),
        dial: Arc::new(crate::network::host_io::NativeDial),
    });
    let uid = app.active.bind().unwrap().uid;
    mode(&app, &p, Mode::Emulation).await.unwrap();
    let status = mode(&app, &p, Mode::Host).await.unwrap();
    assert_eq!(status.mode, Mode::Host);
    assert_eq!(status.host_flows, 0);
    let n = fake.calls.lock().len();
    mode(&app, &p, Mode::Host).await.unwrap();
    assert_eq!(fake.calls.lock().len(), n);
    fake.deliver(&uid, &discover());
    until(|| fake.devices.lock()[&uid].frames.len() == 1).await;
    let reply = fake.devices.lock()[&uid].frames[0].clone();
    assert!(reply[282..]
        .windows(6)
        .any(|w| w == [6, 4, 192, 168, 127, 1]));
    mode(&app, &p, Mode::Emulation).await.unwrap();
    fake.deliver(&uid, &discover());
    until(|| fake.devices.lock()[&uid].frames.len() == 2).await;
    let reply = fake.devices.lock()[&uid].frames[1].clone();
    assert!(!reply[282..]
        .windows(6)
        .any(|w| w == [6, 4, 192, 168, 127, 1]));
    app.network.shutdown().await;
    assert!(!fake.devices.lock()[&uid].enabled);
}

fn scan_request(start: u16, end: u16) -> scanner::Request {
    scanner::Request {
        target: "192.168.127.100".into(),
        start,
        end,
        timeout_ms: 250,
    }
}
fn arp_response(request: &[u8]) -> Vec<u8> {
    let mut b = request.to_vec();
    b[..6].copy_from_slice(&request[6..12]);
    b[6..12].copy_from_slice(&[2, 17, 34, 51, 68, 85]);
    b[20..22].copy_from_slice(&2u16.to_be_bytes());
    b[22..28].copy_from_slice(&[2, 17, 34, 51, 68, 85]);
    b[28..32].copy_from_slice(&request[38..42]);
    b[32..38].copy_from_slice(&request[22..28]);
    b[38..42].copy_from_slice(&request[28..32]);
    b
}
#[tokio::test]
async fn scan_uses_original_channel_and_reports_actual_syn_results_through_the_manager() {
    let (_dir, app, fake, p) = setup();
    let b = app.active.bind().unwrap();
    assert_eq!(
        app.network
            .scan(b.clone(), &p, Some(scan_request(22, 24)))
            .await
            .err()
            .unwrap()
            .code,
        "NETWORK_NOT_READY"
    );
    mode(&app, &p, Mode::Emulation).await.unwrap();
    let s = app
        .network
        .scan(b.clone(), &p, Some(scan_request(22, 24)))
        .await
        .unwrap();
    assert!(s.running);
    until(|| fake.devices.lock()[&b.uid].frames.len() == 1).await;
    let arp = fake.devices.lock()[&b.uid].frames[0].clone();
    fake.deliver(&b.uid, &arp_response(&arp));
    until(|| fake.devices.lock()[&b.uid].frames.len() >= 4).await;
    let frames = fake.devices.lock()[&b.uid].frames.clone();
    for frame in &frames[1..] {
        let parsed = super::super::ipv4::packet(frame).unwrap();
        if parsed.key.remote_port == 24 {
            continue;
        }
        let seq = u32::from_be_bytes(frame[38..42].try_into().unwrap());
        let flags = if parsed.key.remote_port == 22 {
            0x12
        } else {
            0x14
        };
        let reply = super::super::ipv4::tcp_frame(
            [2, 17, 34, 51, 68, 85],
            frame[6..12].try_into().unwrap(),
            super::super::ipv4::Key {
                protocol: 6,
                client: parsed.key.remote,
                client_port: parsed.key.remote_port,
                remote: parsed.key.client,
                remote_port: parsed.key.client_port,
            },
            1234,
            seq.wrapping_add(1),
            flags,
        );
        fake.deliver(&b.uid, &reply);
    }
    until(|| app.network.status(&b).scan.phase == "completed").await;
    let s = app.network.status(&b);
    assert_eq!(s.scan.open, [22]);
    assert_eq!(s.scan.closed, 1);
    assert_eq!(s.scan.filtered, 1);
    assert_eq!(s.scan.completed, 3);
    assert!(s.mcu.enabled);
    assert!(s.error.is_none());
    assert!(fake
        .calls
        .lock()
        .iter()
        .filter(|call| call.op == 0x402)
        .all(|call| call.uid == b.uid && call.session == 7));
    app.network.shutdown().await;
}

#[tokio::test]
async fn scan_stop_and_key_revocation_cancel_a_frame_already_waiting_for_dispatch_without_stopping_ncm(
) {
    for revoke in [false, true] {
        let (_dir, app, fake, p) = setup();
        let b = app.active.bind().unwrap();
        mode(&app, &p, Mode::Emulation).await.unwrap();
        fake.hold_frame.store(true, Ordering::Release);
        app.network
            .scan(b.clone(), &p, Some(scan_request(1, 100)))
            .await
            .unwrap();
        until(|| fake.frame_waiting.load(Ordering::Acquire)).await;
        if revoke {
            p.cancelled.cancel();
            until(|| app.network.status(&b).scan.phase == "cancelled").await;
        } else {
            assert_eq!(
                app.network.scan(b.clone(), &p, None).await.unwrap().phase,
                "stopped"
            );
        }
        fake.hold_frame.store(false, Ordering::Release);
        fake.resume.notify_one();
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(fake.devices.lock()[&b.uid].frames.is_empty());
        assert!(fake.calls.lock().iter().all(|call| call.op != 0x402));
        let s = app.network.status(&b);
        assert!(s.mcu.enabled);
        assert_eq!(s.mode, Mode::Emulation);
        assert!(s.error.is_none());
        assert_eq!(s.scan.completed, 0);
        app.network.shutdown().await;
    }
}

#[tokio::test]
async fn switching_channels_stops_scanning_and_does_not_carry_the_request_to_the_new_channel() {
    let (_dir, app, fake, p) = setup();
    let b = app.active.bind().unwrap();
    mode(&app, &p, Mode::Emulation).await.unwrap();
    app.network
        .scan(b.clone(), &p, Some(scan_request(1, 100)))
        .await
        .unwrap();
    until(|| !fake.devices.lock()[&b.uid].frames.is_empty()).await;
    app.select(Some(Uid::from_bytes(&[2; 12]).unwrap()))
        .await
        .unwrap();
    let count = fake
        .calls
        .lock()
        .iter()
        .filter(|call| call.op == 0x402)
        .count();
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(
        fake.calls
            .lock()
            .iter()
            .filter(|call| call.op == 0x402)
            .count(),
        count
    );
    assert!(!fake.devices.lock()[&b.uid].enabled);
    mode(&app, &p, Mode::Emulation).await.unwrap();
    let new = app.active.bind().unwrap();
    assert_eq!(app.network.status(&new).scan.phase, "idle");
    assert!(fake.devices.lock()[&new.uid].frames.is_empty());
    app.network.shutdown().await;
}

async fn forward_rule() -> forwarding::Rule {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    forwarding::Rule {
        id: "test-rule".into(),
        protocol: forwarding::Protocol::Tcp,
        listen_port: port,
        target_ip: "192.168.127.100".into(),
        target_port: 22,
    }
}
#[tokio::test]
async fn forwarding_rules_apply_without_binding_and_survive_ncm_restart_without_auto_start() {
    let (_dir, app, _fake, p) = setup();
    let b = app.active.bind().unwrap();
    let rule = forward_rule().await;
    let saved = app
        .network
        .configure_forward(&b, &p, vec![rule.clone()])
        .unwrap();
    assert!(!saved.running);
    let probe = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, rule.listen_port))
        .await
        .unwrap();
    drop(probe);
    mode(&app, &p, Mode::Emulation).await.unwrap();
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(
        app.network.status(&b).forwarding.rules[0].listen_port,
        rule.listen_port
    );
    assert!(!app.network.status(&b).forwarding.running);
    app.network
        .forward(b.clone(), p.clone(), Some(vec![rule.clone()]))
        .await
        .unwrap();
    let bad = forwarding::Rule {
        target_ip: "127.0.0.1".into(),
        ..rule.clone()
    };
    assert!(app
        .network
        .forward(b.clone(), p.clone(), Some(vec![bad]))
        .await
        .is_err());
    assert!(app.network.status(&b).forwarding.running);
    mode(&app, &p, Mode::Off).await.unwrap();
    tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, rule.listen_port))
        .await
        .unwrap();
    mode(&app, &p, Mode::Emulation).await.unwrap();
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(app.network.status(&b).forwarding.rules.len(), 1);
    assert!(!app.network.status(&b).forwarding.running);
    app.network.configure_forward(&b, &p, vec![]).unwrap();
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert!(app.network.status(&b).forwarding.rules.is_empty());
    app.shutdown().await;
}

#[tokio::test]
async fn completed_forwarding_survives_owner_closure_but_stop_cancels_a_queued_frame_and_releases_local_port(
) {
    let (_dir, app, fake, p) = setup();
    let b = app.active.bind().unwrap();
    mode(&app, &p, Mode::Emulation).await.unwrap();
    let rule = forward_rule().await;
    app.network
        .forward(b.clone(), p.clone(), Some(vec![rule.clone()]))
        .await
        .unwrap();
    p.ended.cancel();
    fake.hold_frame.store(true, Ordering::Release);
    let local = tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, rule.listen_port))
        .await
        .unwrap();
    until(|| fake.frame_waiting.load(Ordering::Acquire)).await;
    let other = Principal {
        key_id: "other".into(),
        client_id: "other".into(),
        cancelled: CancellationToken::new(),
        ended: CancellationToken::new(),
    };
    let stopped = app.network.forward(b.clone(), other, None).await.unwrap();
    assert!(!stopped.running);
    assert!(!stopped.cleanup_required);
    fake.hold_frame.store(false, Ordering::Release);
    fake.resume.notify_one();
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert!(fake.devices.lock()[&b.uid].enabled);
    assert!(fake.calls.lock().iter().all(|call| call.op != 0x402));
    assert!(app.network.status(&b).error.is_none());
    tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, rule.listen_port))
        .await
        .unwrap();
    drop(local);
    app.shutdown().await;
}

#[tokio::test]
async fn channel_switch_closes_forwarding_before_selecting_another_uid() {
    let (_dir, app, fake, p) = setup();
    let b = app.active.bind().unwrap();
    mode(&app, &p, Mode::Emulation).await.unwrap();
    let rule = forward_rule().await;
    app.network
        .forward(b.clone(), p, Some(vec![rule.clone()]))
        .await
        .unwrap();
    app.select(Some(Uid::from_bytes(&[2; 12]).unwrap()))
        .await
        .unwrap();
    tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, rule.listen_port))
        .await
        .unwrap();
    assert!(!fake.devices.lock()[&b.uid].enabled);
    let status = app.network.status(&app.active.bind().unwrap());
    assert!(!status.forwarding.running);
    assert!(status.forwarding.rules.is_empty());
    app.shutdown().await;
}
