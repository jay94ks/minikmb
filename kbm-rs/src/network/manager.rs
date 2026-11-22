//! One app-owned active NCM resource, with bounded setup work and original-session cleanup.
use super::{
    dhcp::Lease,
    forwarding,
    framing::{self, McuStats, Receiver, Transmitter, Watermark},
    scanner,
    stack::Engine,
    wire::{Backend, Connection, Wire},
    Config, Mode,
};
use crate::{
    active::{ActiveCoordinator, Binding},
    actor::DeviceEvent,
    input_sessions::Principal,
    model::{now_ms, AppError, Uid},
};
use parking_lot::Mutex;
use serde::Serialize;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::sync::{broadcast, mpsc, oneshot, Semaphore};
use tokio_util::sync::CancellationToken;
#[cfg(test)]
pub(crate) mod tests;
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct Status {
    pub uid: Option<Uid>,
    pub generation: u64,
    pub mode: Mode,
    pub phase: String,
    pub config: Config,
    pub mcu: McuStats,
    pub rx_frames: u64,
    pub tx_frames: u64,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub dropped: u64,
    pub tx_queued: usize,
    pub host_flows: usize,
    pub scan: scanner::Status,
    pub forwarding: forwarding::Status,
    pub flow_usage: usize,
    pub leases: Vec<Lease>,
    pub observed_at: Option<u64>,
    pub cleanup_required: bool,
    pub error: Option<AppError>,
    pub cleanup_error: Option<AppError>,
}
impl Status {
    fn off(b: &Binding, config: Config) -> Self {
        Self {
            uid: Some(b.uid.clone()),
            generation: b.generation,
            mode: Mode::Off,
            phase: "off".into(),
            config,
            mcu: Default::default(),
            rx_frames: 0,
            tx_frames: 0,
            rx_bytes: 0,
            tx_bytes: 0,
            dropped: 0,
            tx_queued: 0,
            host_flows: 0,
            scan: Default::default(),
            forwarding: Default::default(),
            flow_usage: 0,
            leases: vec![],
            observed_at: None,
            cleanup_required: false,
            error: None,
            cleanup_error: None,
        }
    }
}
struct Session {
    id: u64,
    wire: Arc<dyn Wire>,
    tx: Mutex<Transmitter>,
    floor: Mutex<Watermark>,
}
struct Run {
    binding: Binding,
    session: Arc<Session>,
    life: CancellationToken,
    status: Mutex<Status>,
    worker: Mutex<Option<tokio::task::JoinHandle<()>>>,
    commands: Mutex<Option<mpsc::Sender<Control>>>,
    scan_cancel: Mutex<Option<CancellationToken>>,
    forward_commands: Mutex<Option<mpsc::Sender<ForwardControl>>>,
    forward_cancel: Mutex<Option<CancellationToken>>,
}
struct ForwardControl {
    bound: Option<forwarding::Bound>,
    binding: Binding,
    owner: Principal,
    reply: oneshot::Sender<Result<forwarding::Status, AppError>>,
}
struct Control {
    request: Option<scanner::Request>,
    binding: Binding,
    owner: Principal,
    reply: oneshot::Sender<Result<scanner::Status, AppError>>,
}
#[derive(Default)]
struct State {
    current: Option<Arc<Run>>,
    last: Option<Status>,
    sessions: HashMap<Uid, Arc<Session>>,
}
pub struct Manager {
    preferences: Arc<crate::preferences::Store>,
    active: Arc<ActiveCoordinator>,
    backend: Arc<dyn Backend>,
    state: Mutex<State>,
    gate: tokio::sync::Mutex<()>,
    controls: Arc<Semaphore>,
    stopped: AtomicBool,
}
fn owner_check(p: &Principal) -> Result<(), AppError> {
    if p.cancelled.is_cancelled() || p.ended.is_cancelled() {
        Err(AppError::new(
            "UNAUTHORIZED",
            "네트워크 제어 연결이 종료됐습니다.",
        ))
    } else {
        Ok(())
    }
}
fn invalid() -> AppError {
    AppError::new(
        "INVALID_RESPONSE",
        "NCM 응답의 길이·상태·credit을 확인하지 못했습니다.",
    )
}
fn cleanup_error() -> AppError {
    AppError::uncertain(
        "NETWORK_CLEANUP_REQUIRED",
        "원래 채널의 NCM OFF를 확인하지 못했습니다. 원래 채널에서 네트워크 끄기로 복구하세요.",
    )
}
async fn request(
    run: &Run,
    owner: &Principal,
    ch: u8,
    op: u16,
    p: Vec<u8>,
) -> Result<Vec<u8>, AppError> {
    request_scoped(run, owner, run.life.clone(), ch, op, p).await
}
async fn request_scoped(
    run: &Run,
    owner: &Principal,
    life: CancellationToken,
    ch: u8,
    op: u16,
    p: Vec<u8>,
) -> Result<Vec<u8>, AppError> {
    owner_check(owner)?;
    if run.life.is_cancelled() || life.is_cancelled() {
        return Err(AppError::new(
            "OPERATION_CANCELLED",
            "NCM 작업을 취소했습니다.",
        ));
    }
    tokio::time::timeout(
        Duration::from_secs(3),
        run.session
            .wire
            .send(run.binding.clone(), owner.clone(), life.clone(), ch, op, p),
    )
    .await
    .map_err(|_| {
        AppError::uncertain(
            "NETWORK_TIMEOUT",
            "NCM 요청의 완료를 확인하지 못했습니다. 프레임을 자동 재전송하지 않습니다.",
        )
    })?
}
async fn ack(run: &Run, owner: &Principal, op: u16, p: Vec<u8>) -> Result<(), AppError> {
    if !request(run, owner, 4, op, p).await?.is_empty() {
        return Err(invalid());
    }
    Ok(())
}
async fn verified_off(wire: &Arc<dyn Wire>) -> Result<McuStats, AppError> {
    let result = tokio::time::timeout(Duration::from_secs(4), async {
        if !wire.live() {
            return Err(cleanup_error());
        }
        if !wire.cleanup(0x401).await?.is_empty() {
            return Err(invalid());
        }
        let stats = McuStats::decode(&wire.cleanup(0x403).await?)?;
        if stats.enabled || stats.link_up {
            return Err(cleanup_error());
        }
        Ok(stats)
    })
    .await
    .map_err(|_| cleanup_error())?;
    result
}
impl Manager {
    pub fn configure_forward(
        &self,
        b: &Binding,
        owner: &Principal,
        rules: Vec<forwarding::Rule>,
    ) -> Result<forwarding::Status, AppError> {
        let _gate = self
            .gate
            .try_lock()
            .map_err(|_| AppError::new("NETWORK_BUSY", "네트워크 모드 변경 중입니다."))?;
        owner_check(owner)?;
        self.active.validate(b)?;
        let config = self.status(b).config;
        if !rules.is_empty() {
            forwarding::validate(&rules, &config)?;
        }
        if self
            .state
            .lock()
            .current
            .as_ref()
            .is_some_and(|run| run.status.lock().forwarding.cleanup_required)
        {
            return Err(AppError::new("FORWARD_BUSY", "포워딩을 먼저 중지하세요."));
        }
        self.preferences.forwards(&b.uid, rules.clone())?;
        self.active.validate(b)?;
        let mut state = self.state.lock();
        let status = forwarding::Status {
            rules,
            ..Default::default()
        };
        if let Some(run) = &state.current {
            if run.binding == *b {
                run.status.lock().forwarding = status.clone();
            }
        }
        if let Some(last) = &mut state.last {
            if last.uid.as_ref() == Some(&b.uid) {
                last.forwarding = status.clone();
            }
        }
        Ok(status)
    }
    pub async fn forward(
        self: &Arc<Self>,
        b: Binding,
        owner: Principal,
        rules: Option<Vec<forwarding::Rule>>,
    ) -> Result<forwarding::Status, AppError> {
        owner_check(&owner)?;
        self.active.validate(&b)?;
        let permit = self
            .controls
            .clone()
            .try_acquire_owned()
            .map_err(|_| AppError::new("QUEUE_FULL", "네트워크 제어 요청이 너무 많습니다."))?;
        let manager = self.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let _gate = manager.gate.lock().await;
            manager.forward_inner(b, owner, rules).await
        })
        .await
        .map_err(|_| {
            AppError::uncertain("FORWARD_FAILED", "포워딩 변경 결과를 확인하지 못했습니다.")
        })?
    }
    async fn forward_inner(
        &self,
        b: Binding,
        owner: Principal,
        rules: Option<Vec<forwarding::Rule>>,
    ) -> Result<forwarding::Status, AppError> {
        owner_check(&owner)?;
        self.active.validate(&b)?;
        let run = self
            .state
            .lock()
            .current
            .clone()
            .filter(|run| run.binding == b && !run.life.is_cancelled() && run.session.wire.live())
            .ok_or_else(|| AppError::new("NETWORK_NOT_READY", "선택 채널의 NCM을 먼저 켜세요."))?;
        let config = run.status.lock().config.clone();
        if let Some(rules) = &rules {
            forwarding::validate(rules, &config)?;
            if !run.status.lock().mcu.link_up {
                return Err(AppError::new(
                    "NETWORK_NOT_READY",
                    "타깃 NCM 연결을 확인하세요.",
                ));
            }
        }
        if let Some(token) = run.forward_cancel.lock().take() {
            token.cancel();
        }
        let internal = Principal {
            key_id: "app-network".into(),
            client_id: "app-network".into(),
            cancelled: run.life.clone(),
            ended: CancellationToken::new(),
        };
        let stopped = self.apply_forward(&run, internal.clone(), None).await?;
        let Some(rules) = rules else {
            return Ok(stopped);
        };
        owner_check(&owner)?;
        self.active.validate(&b)?;
        let bound = match tokio::time::timeout(
            Duration::from_secs(3),
            forwarding::Bound::bind(rules.clone(), &config),
        )
        .await
        {
            Ok(Ok(bound)) => bound,
            result => {
                let e = match result {
                    Ok(Err(e)) => e,
                    _ => {
                        AppError::new("FORWARD_BIND_FAILED", "로컬 포트 준비 시간이 초과됐습니다.")
                    }
                };
                run.status.lock().forwarding.error = Some(e.clone());
                return Err(e);
            }
        };
        owner_check(&owner)?;
        self.active.validate(&b)?;
        let status = self.apply_forward(&run, owner.clone(), Some(bound)).await?;
        if let Err(e) = owner_check(&owner).and_then(|_| self.active.validate(&b)) {
            if let Some(token) = run.forward_cancel.lock().take() {
                token.cancel();
            }
            let _ = self.apply_forward(&run, internal, None).await;
            return Err(e);
        }
        let preferences = self.preferences.clone();
        let uid = b.uid.clone();
        let saved = tokio::task::spawn_blocking(move || preferences.forwards(&uid, rules))
            .await
            .unwrap_or_else(|_| {
                Err(AppError::new(
                    "PREFERENCES_IO",
                    "포워딩 설정을 저장하지 못했습니다.",
                ))
            });
        if let Err(error) = saved {
            if let Some(token) = run.forward_cancel.lock().take() {
                token.cancel();
            }
            let _ = self.apply_forward(&run, internal, None).await;
            return Err(error);
        }
        Ok(status)
    }
    async fn apply_forward(
        &self,
        run: &Run,
        owner: Principal,
        bound: Option<forwarding::Bound>,
    ) -> Result<forwarding::Status, AppError> {
        let sender = run
            .forward_commands
            .lock()
            .clone()
            .ok_or_else(|| AppError::new("NETWORK_STOPPED", "NCM 워커가 종료됐습니다."))?;
        let (reply, rx) = oneshot::channel();
        sender
            .try_send(ForwardControl {
                bound,
                binding: run.binding.clone(),
                owner,
                reply,
            })
            .map_err(|_| AppError::new("QUEUE_FULL", "포워딩 제어 큐가 가득 찼습니다."))?;
        tokio::time::timeout(Duration::from_secs(4), rx)
            .await
            .map_err(|_| {
                AppError::uncertain(
                    "FORWARD_TIMEOUT",
                    "포워딩 정리/시작 결과를 확인하지 못했습니다.",
                )
            })?
            .map_err(|_| AppError::new("NETWORK_STOPPED", "NCM 워커가 종료됐습니다."))?
    }
    pub async fn scan(
        &self,
        b: Binding,
        owner: &Principal,
        request: Option<scanner::Request>,
    ) -> Result<scanner::Status, AppError> {
        let _gate = self
            .gate
            .try_lock()
            .map_err(|_| AppError::new("NETWORK_BUSY", "네트워크 모드를 변경하고 있습니다."))?;
        owner_check(owner)?;
        self.active.validate(&b)?;
        let run = self
            .state
            .lock()
            .current
            .clone()
            .filter(|run| run.binding == b && !run.life.is_cancelled() && run.session.wire.live())
            .ok_or_else(|| AppError::new("NETWORK_NOT_READY", "선택 채널의 NCM을 먼저 켜세요."))?;
        {
            let status = run.status.lock();
            if !status.mcu.enabled || request.is_some() && !status.mcu.link_up {
                return Err(AppError::new(
                    "NETWORK_NOT_READY",
                    "선택 타깃의 NCM 연결을 확인하세요.",
                ));
            }
            if let Some(request) = &request {
                request.validate(&status.config)?;
            }
        }
        let sender = run
            .commands
            .lock()
            .clone()
            .ok_or_else(|| AppError::new("NETWORK_NOT_READY", "NCM 워커를 기다려 주세요."))?;
        if request.is_none() {
            if let Some(token) = run.scan_cancel.lock().take() {
                token.cancel();
            }
        }
        let (reply, rx) = oneshot::channel();
        sender
            .try_send(Control {
                request,
                binding: b,
                owner: owner.clone(),
                reply,
            })
            .map_err(|_| AppError::new("QUEUE_FULL", "네트워크 제어 큐가 가득 찼습니다."))?;
        tokio::time::timeout(Duration::from_secs(4), rx)
            .await
            .map_err(|_| AppError::new("NETWORK_BUSY", "스캔 제어 응답을 기다리지 못했습니다."))?
            .map_err(|_| AppError::new("NETWORK_STOPPED", "NCM 워커가 종료됐습니다."))?
    }
    pub fn new(active: Arc<ActiveCoordinator>, backend: Arc<dyn Backend>) -> Arc<Self> {
        Self::with_preferences(active, backend, crate::preferences::Store::memory())
    }
    pub fn with_preferences(
        active: Arc<ActiveCoordinator>,
        backend: Arc<dyn Backend>,
        preferences: Arc<crate::preferences::Store>,
    ) -> Arc<Self> {
        Arc::new(Self {
            preferences,
            active,
            backend,
            state: Default::default(),
            gate: Default::default(),
            controls: Arc::new(Semaphore::new(4)),
            stopped: AtomicBool::new(false),
        })
    }
    pub fn status(&self, b: &Binding) -> Status {
        let saved = self.preferences.channel(&b.uid);
        let s = self.state.lock();
        if let Some(run) = &s.current {
            if run.binding.uid == b.uid {
                let mut status = run.status.lock().clone();
                if status.mode == Mode::Off && !status.cleanup_required {
                    status.config = saved.config();
                    status.forwarding.rules = saved.rules();
                }
                return status;
            }
        }
        if let Some(last) = &s.last {
            if last.uid.as_ref() == Some(&b.uid) && last.generation == b.generation {
                let mut status = last.clone();
                if status.mode == Mode::Off && !status.cleanup_required {
                    status.config = saved.config();
                    status.forwarding.rules = saved.rules();
                }
                return status;
            }
        }
        let mut status = Status::off(b, saved.config());
        status.forwarding.rules = saved.rules();
        status
    }
    pub fn resource(&self) -> Option<Status> {
        self.state
            .lock()
            .current
            .as_ref()
            .map(|r| r.status.lock().clone())
    }
    pub fn configure(
        &self,
        b: &Binding,
        owner: &Principal,
        config: Config,
    ) -> Result<Status, AppError> {
        let _gate = self
            .gate
            .try_lock()
            .map_err(|_| AppError::new("NETWORK_BUSY", "네트워크 모드 변경 중입니다."))?;
        owner_check(owner)?;
        self.active.validate(b)?;
        config.validate()?;
        if self.stopped.load(Ordering::Acquire) {
            return Err(AppError::new("OPERATION_CANCELLED", "앱이 종료 중입니다."));
        }
        if self
            .state
            .lock()
            .current
            .as_ref()
            .is_some_and(|r| r.status.lock().cleanup_required)
        {
            return Err(AppError::new(
                "NETWORK_BUSY",
                "네트워크를 끄고 설정을 변경하세요.",
            ));
        }
        self.preferences.network(&b.uid, config.clone())?;
        self.active.validate(b)?;
        let mut s = self.state.lock();
        s.current = None;
        let result = Status::off(b, config);
        s.last = Some(result.clone());
        Ok(result)
    }
    pub async fn set_mode(
        self: &Arc<Self>,
        b: Binding,
        owner: Principal,
        mode: Mode,
    ) -> Result<Status, AppError> {
        owner_check(&owner)?;
        self.active.validate(&b)?;
        let permit = self
            .controls
            .clone()
            .try_acquire_owned()
            .map_err(|_| AppError::new("QUEUE_FULL", "네트워크 제어 요청이 너무 많습니다."))?;
        let manager = self.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let _gate = manager.gate.lock().await;
            manager.set_inner(b, owner, mode).await
        })
        .await
        .map_err(|_| {
            AppError::uncertain(
                "NETWORK_FAILED",
                "네트워크 모드 변경 결과를 확인하지 못했습니다.",
            )
        })?
    }
    async fn set_inner(
        self: &Arc<Self>,
        b: Binding,
        owner: Principal,
        mode: Mode,
    ) -> Result<Status, AppError> {
        owner_check(&owner)?;
        self.active.validate(&b)?;
        if self.stopped.load(Ordering::Acquire) {
            return Err(AppError::new("OPERATION_CANCELLED", "앱이 종료 중입니다."));
        }
        if let Some(run) = self
            .state
            .lock()
            .current
            .as_ref()
            .filter(|r| r.binding == b && r.session.wire.live() && !r.life.is_cancelled())
        {
            let status = run.status.lock();
            if status.mode == mode
                && status.mcu.enabled
                && status.error.is_none()
                && ["running", "waiting_target"].contains(&status.phase.as_str())
            {
                return Ok(status.clone());
            }
        }
        // Resolve host setup before altering an already working emulation link.
        let host = if mode == Mode::Host {
            Some(
                tokio::time::timeout(Duration::from_secs(3), self.backend.host())
                    .await
                    .map_err(|_| {
                        AppError::new("DNS_UNAVAILABLE", "관리 PC DNS 조회 시간이 초과됐습니다.")
                    })??,
            )
        } else {
            None
        };
        owner_check(&owner)?;
        self.active.validate(&b)?;
        self.release(Some((&b, &owner)), None).await?;
        owner_check(&owner)?;
        self.active.validate(&b)?;
        let config = self.preferences.channel(&b.uid).config();
        let conn = self.backend.connect(&b)?;
        let session = {
            let mut s = self.state.lock();
            s.sessions.retain(|_, session| session.wire.live());
            if let Some(session) = s
                .sessions
                .get(&b.uid)
                .filter(|session| session.id == conn.session)
            {
                session.clone()
            } else {
                if s.sessions.len() >= 64 {
                    return Err(AppError::new(
                        "QUEUE_FULL",
                        "NCM 세션 보관 한도64개를 초과했습니다.",
                    ));
                }
                let session = Arc::new(Session {
                    id: conn.session,
                    wire: conn.wire.clone(),
                    tx: Mutex::new(Transmitter::new(conn.session)?),
                    floor: Default::default(),
                });
                s.sessions.insert(b.uid.clone(), session.clone());
                session
            }
        };
        let saved_rules = self.preferences.channel(&b.uid).rules();
        let run = Arc::new(Run {
            binding: b.clone(),
            session,
            life: CancellationToken::new(),
            status: Mutex::new(Status {
                mode,
                phase: "starting".into(),
                cleanup_required: true,
                forwarding: forwarding::Status {
                    rules: saved_rules,
                    ..Default::default()
                },
                ..Status::off(&b, config.clone())
            }),
            worker: Default::default(),
            commands: Default::default(),
            scan_cancel: Default::default(),
            forward_commands: Default::default(),
            forward_cancel: Default::default(),
        });
        self.state.lock().current = Some(run.clone());
        let result = self.start(&run, &owner, mode, config, conn, host).await;
        match result {
            Ok(status) => Ok(status),
            Err(error) => {
                run.life.cancel();
                run.status.lock().error = Some(error.clone());
                self.cleanup(&run).await;
                Err(error)
            }
        }
    }
    async fn start(
        self: &Arc<Self>,
        run: &Arc<Run>,
        owner: &Principal,
        mode: Mode,
        config: Config,
        mut conn: Connection,
        host: Option<super::host_io::Host>,
    ) -> Result<Status, AppError> {
        self.backend
            .prepare_target_change(run.binding.clone())
            .await?;
        self.active.validate(&run.binding)?;
        ack(run, owner, 0x401, vec![0]).await?;
        let off = McuStats::decode(&request(run, owner, 4, 0x403, vec![]).await?)?;
        if off.enabled || off.link_up {
            return Err(invalid());
        }
        if mode == Mode::Off {
            let mut status = run.status.lock();
            status.mcu = off;
            status.mode = Mode::Off;
            status.phase = "off".into();
            status.cleanup_required = false;
            status.observed_at = Some(now_ms());
            return Ok(status.clone());
        }
        let (sent, old_limit) =
            framing::credit_reply(&request(run, owner, 0, 6, framing::credit(0)).await?)?;
        loop {
            match conn.events.try_recv() {
                Ok(event) => {
                    if event.uid == run.binding.uid
                        && event.session == run.session.id
                        && event.frame.session == run.session.id
                        && event.frame.kind == 4
                        && event.frame.channel == 4
                        && event.frame.opcode == 0x480
                    {
                        run.session.floor.lock().observe(&event.frame);
                    }
                }
                Err(broadcast::error::TryRecvError::Lagged(_)) => {}
                Err(broadcast::error::TryRecvError::Empty) => break,
                Err(_) => return Err(cleanup_error()),
            }
        }
        let limit = sent
            .checked_add(framing::WINDOW)
            .ok_or_else(invalid)?
            .max(old_limit);
        let rx = Receiver::resumed(run.session.id, sent, limit, *run.session.floor.lock())?;
        let (accepted, granted) =
            framing::credit_reply(&request(run, owner, 0, 6, framing::credit(limit)).await?)?;
        if accepted != sent || granted != limit {
            return Err(invalid());
        }
        let mut mac = [2, 0x4b, 0x42, 0, 0, 0];
        let hash = crc32fast::hash(run.binding.uid.raw().as_bytes()).to_le_bytes();
        mac[3..].copy_from_slice(&hash[..3]);
        let engine = if let Some(host) = host {
            Engine::with_host_scoped(config, mac, host, run.life.clone())?
        } else {
            Engine::new(config, mac, false)?
        };
        ack(run, owner, 0x401, vec![1]).await?;
        let stats = McuStats::decode(&request(run, owner, 4, 0x403, vec![]).await?)?;
        owner_check(owner)?;
        self.active.validate(&run.binding)?;
        if !stats.enabled {
            return Err(AppError::new(
                "NETWORK_NOT_READY",
                "MCU가 NCM을 켜지 못했습니다.",
            ));
        }
        {
            let mut status = run.status.lock();
            status.mcu = stats;
            status.phase = if status.mcu.link_up {
                "running"
            } else {
                "waiting_target"
            }
            .into();
            status.observed_at = Some(now_ms());
        }
        let manager = self.clone();
        let worker_run = run.clone();
        let events = conn.events;
        let (sender, commands) = mpsc::channel(8);
        *run.commands.lock() = Some(sender);
        let (sender, forward_commands) = mpsc::channel(8);
        *run.forward_commands.lock() = Some(sender);
        *run.worker.lock() = Some(tokio::spawn(async move {
            manager
                .worker(worker_run, events, rx, engine, commands, forward_commands)
                .await;
        }));
        Ok(run.status.lock().clone())
    }
    async fn cleanup(&self, run: &Run) {
        run.life.cancel();
        {
            let mut s = run.status.lock();
            s.phase = "stopping".into();
            s.tx_queued = 0;
            s.host_flows = 0;
            s.flow_usage = 0;
            s.forwarding.running = false;
            s.forwarding.cleanup_required = false;
            s.forwarding.connections = 0;
            if s.scan.running {
                s.scan.running = false;
                s.scan.phase = "stopped".into();
                s.scan.pending = 0;
            }
        }
        let result = match self
            .backend
            .prepare_target_change(run.binding.clone())
            .await
        {
            Ok(()) => verified_off(&run.session.wire).await,
            Err(e) => Err(e),
        };
        let mut s = run.status.lock();
        match result {
            Ok(stats) => {
                s.mcu = stats;
                s.mode = Mode::Off;
                s.cleanup_required = false;
                s.cleanup_error = None;
                s.phase = if s.error.is_some() { "failed" } else { "off" }.into();
                s.observed_at = Some(now_ms());
            }
            Err(e) => {
                s.cleanup_required = true;
                s.cleanup_error = Some(e);
                s.phase = "failed".into();
            }
        }
    }
    async fn release(
        &self,
        recovery: Option<(&Binding, &Principal)>,
        target: Option<&Uid>,
    ) -> Result<(), AppError> {
        let Some(run) = self.state.lock().current.clone() else {
            return Ok(());
        };
        run.life.cancel();
        let worker = run.worker.lock().take();
        if let Some(mut worker) = worker {
            match tokio::time::timeout(Duration::from_secs(6), &mut worker).await {
                Err(_) => {
                    *run.worker.lock() = Some(worker);
                    return Err(cleanup_error());
                }
                Ok(Err(_)) => {
                    run.status.lock().error = Some(AppError::uncertain(
                        "NETWORK_FAILED",
                        "NCM 워커가 종료됐습니다. OFF 확인이 필요합니다.",
                    ));
                }
                Ok(Ok(())) => {}
            }
        }
        if run.status.lock().cleanup_required {
            if !run.session.wire.live() {
                if target == Some(&run.binding.uid) {
                    return Ok(());
                }
                let Some((b, owner)) = recovery.filter(|(b, _)| b.uid == run.binding.uid) else {
                    return Err(cleanup_error());
                };
                owner_check(owner)?;
                self.active.validate(b)?;
                let conn = self.backend.connect(b)?;
                if conn.session == run.session.id {
                    return Err(cleanup_error());
                }
                self.backend.prepare_target_change(b.clone()).await?;
                let life = CancellationToken::new();
                let reply = conn
                    .wire
                    .send(b.clone(), owner.clone(), life.clone(), 4, 0x401, vec![0])
                    .await?;
                if !reply.is_empty() {
                    return Err(invalid());
                }
                let stats = McuStats::decode(
                    &conn
                        .wire
                        .send(b.clone(), owner.clone(), life, 4, 0x403, vec![])
                        .await?,
                )?;
                if stats.enabled || stats.link_up {
                    return Err(cleanup_error());
                }
                let mut status = run.status.lock();
                status.mcu = stats;
                status.mode = Mode::Off;
                status.cleanup_required = false;
                status.cleanup_error = None;
                status.phase = "off".into();
            } else {
                self.cleanup(&run).await;
                if run.status.lock().cleanup_required {
                    return Err(cleanup_error());
                }
            }
        }
        let mut state = self.state.lock();
        state.last = Some(run.status.lock().clone());
        state.current = None;
        Ok(())
    }
    pub async fn prepare_switch(&self, target: Option<&Uid>) -> Result<(), AppError> {
        let _gate = self.gate.lock().await;
        self.release(None, target).await
    }
    pub fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
        if let Some(run) = &self.state.lock().current {
            run.life.cancel();
        }
    }
    pub fn quiesce(&self) {
        if let Some(run) = &self.state.lock().current {
            run.life.cancel();
        }
    }
    pub async fn shutdown(&self) {
        self.stop();
        let _gate = self.gate.lock().await;
        let _ = self.release(None, None).await;
    }
    async fn worker(
        self: Arc<Self>,
        run: Arc<Run>,
        mut events: broadcast::Receiver<DeviceEvent>,
        mut rx: Receiver,
        mut engine: Engine,
        mut commands: mpsc::Receiver<Control>,
        mut forward_commands: mpsc::Receiver<ForwardControl>,
    ) {
        let owner = Principal {
            key_id: "app-network".into(),
            client_id: "app-network".into(),
            cancelled: run.life.clone(),
            ended: CancellationToken::new(),
        };
        let (tx, mut pending) = mpsc::channel::<scanner::Output>(32);
        let sender_run = run.clone();
        let sender_owner = owner.clone();
        let sender = tokio::spawn(async move {
            loop {
                let frame = tokio::select! {biased;_=sender_run.life.cancelled()=>break,frame=pending.recv()=>match frame{Some(frame)=>frame,None=>break}};
                if frame.scope.as_ref().is_some_and(scanner::Scope::cancelled) {
                    continue;
                }
                let parts = sender_run
                    .session
                    .tx
                    .lock()
                    .prepare(sender_run.session.id, &frame.frame);
                let result = async {
                    for p in parts? {
                        let (owner, life) = frame
                            .scope
                            .as_ref()
                            .map(|scope| (&scope.owner, scope.life.clone()))
                            .unwrap_or((&sender_owner, sender_run.life.clone()));
                        if !request_scoped(&sender_run, owner, life, 4, 0x402, p)
                            .await?
                            .is_empty()
                        {
                            return Err(invalid());
                        }
                    }
                    Ok::<_, AppError>(())
                }
                .await;
                match result {
                    Ok(()) => {
                        let mut s = sender_run.status.lock();
                        s.tx_frames += 1;
                        s.tx_bytes += frame.frame.len() as u64;
                    }
                    Err(e) => {
                        if frame.scope.as_ref().is_some_and(scanner::Scope::cancelled) {
                            continue;
                        }
                        if !sender_run.life.is_cancelled() {
                            sender_run.status.lock().error = Some(e);
                        }
                        sender_run.life.cancel();
                        break;
                    }
                }
            }
        });
        let began = Instant::now();
        let mut ticker = tokio::time::interval(Duration::from_millis(5));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut stats_due = Instant::now() + Duration::from_millis(500);
        let mut scan_revision = 0;
        let result=async {
            loop {
                if !run.session.wire.live(){return Err(cleanup_error());}
                self.active.validate(&run.binding)?;
                let now=began.elapsed().as_millis().min(u64::MAX as u128) as u64;
                tokio::select!{biased;
                    _=run.life.cancelled()=>break,
                    Some(control)=forward_commands.recv()=>{
                        if !control.reply.is_closed() {
                            let result=async {
                                owner_check(&control.owner)?;self.active.validate(&control.binding)?;
                                if control.binding!=run.binding {return Err(AppError::new("STALE_CHANNEL","원래 포워딩 채널이 변경됐습니다."));}
                                if let Some(bound)=control.bound {
                                    if !run.status.lock().mcu.link_up {return Err(AppError::new("NETWORK_NOT_READY","타깃 NCM 연결이 끊겼습니다."));}
                                    let token=engine.start_forward(bound,owner.clone(),run.life.clone())?;*run.forward_cancel.lock()=Some(token);
                                }else {engine.stop_forward().finish().await;}
                                let mut status=engine.forward_status();
                                if !status.cleanup_required {status.rules=self.preferences.channel(&run.binding.uid).rules();}
                                run.status.lock().forwarding=status.clone();Ok(status)
                            }.await;let _=control.reply.send(result);
                        }
                    },
                    Some(control)=commands.recv()=>{
                        if !control.reply.is_closed() {
                            let result=(|| {owner_check(&control.owner)?;self.active.validate(&control.binding)?;
                                if control.binding!=run.binding {return Err(AppError::new("STALE_CHANNEL","원래 스캔 채널이 변경됐습니다."));}
                                if let Some(request)=control.request {
                                    if !run.status.lock().mcu.link_up {return Err(AppError::new("NETWORK_NOT_READY","타깃의 NCM 링크가 끊겼습니다."));}
                                    let token=engine.start_scan(request,control.owner,run.life.clone())?;*run.scan_cancel.lock()=Some(token);
                                }
                                else {engine.stop_scan();}
                                Ok(engine.scan_status().clone())
                            })();let _=control.reply.send(result);
                        }
                    },
                    event=events.recv()=>{let event=event.map_err(|_|AppError::new("NETWORK_STREAM_LOST","NCM 이벤트 수신이 끊기거나 버퍼가 넘쳤습니다."))?;
                        if event.uid==run.binding.uid&&event.session==run.session.id {
                            if let Some(frame)=rx.feed(&event.frame,Instant::now())? {let len=frame.len();engine.receive(frame,now);let mut s=run.status.lock();s.rx_frames+=1;s.rx_bytes+=len as u64;}
                            *run.session.floor.lock()=rx.watermark();
                        }
                    },
                    _=ticker.tick()=>{}
                }
                if run.life.is_cancelled(){break;}
                rx.expire(Instant::now());engine.poll(now);
                while engine.output_len()!=0 {let Ok(permit)=tx.try_reserve() else{break;};permit.send(engine.take_output().unwrap());}
                if engine.scan_revision()!=scan_revision {scan_revision=engine.scan_revision();run.status.lock().scan=engine.scan_status().clone();}
                let mut forward_status=engine.forward_status();
                if !forward_status.cleanup_required {forward_status.rules=self.preferences.channel(&run.binding.uid).rules();forward_status.error=forward_status.error.or_else(||run.status.lock().forwarding.error.clone());}
                {let mut s=run.status.lock();s.dropped=rx.dropped+engine.dropped();s.tx_queued=engine.output_len()+32-tx.capacity();s.host_flows=engine.flows();s.flow_usage=engine.flow_usage();s.forwarding=forward_status;s.leases=engine.leases(now);}
                if let Some(limit)=rx.renewal()? {
                    let (_,granted)=framing::credit_reply(&request(&run,&owner,0,6,framing::credit(limit)).await?)?;
                    if granted!=limit{return Err(invalid());}
                }
                if Instant::now()>=stats_due {
                    let stats=McuStats::decode(&request(&run,&owner,4,0x403,vec![]).await?)?;
                    if !stats.enabled{return Err(AppError::new("NETWORK_STOPPED","MCU가 NCM을 비활성화했습니다."));}
                    if !stats.link_up && engine.scan_status().running {engine.stop_scan();}
                    let mut s=run.status.lock();s.mcu=stats;s.observed_at=Some(now_ms());s.phase=if s.mcu.link_up {"running"}else{"waiting_target"}.into();stats_due=Instant::now()+Duration::from_millis(500);
                }
                // A busy event stream can keep biased select immediately
                // ready. Cooperate after each bounded batch, outside locks.
                tokio::task::yield_now().await;
            } Ok::<_,AppError>(())
        }.await;
        if let Err(e) = result {
            if !run.life.is_cancelled() {
                run.status.lock().error = Some(e);
            }
        }
        run.life.cancel();
        engine.shutdown_io().await;
        drop(engine);
        let _ = sender.await;
        self.cleanup(&run).await;
    }
}
