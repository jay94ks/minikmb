//! One native I/O thread per MCU; no UI thread waits for a COM read.
use crate::{
    active::{ActiveCoordinator, Binding},
    device::Session,
    model::{now_ms, AppError, Channel, Inventory, Uid},
    protocol::Frame,
    transport::{Completion, Effect, Machine},
};
use parking_lot::{Mutex, RwLock};
use std::{
    collections::{HashMap, VecDeque},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio_util::sync::CancellationToken;
static NEXT_IDENTITY: AtomicU64 = AtomicU64::new(1);

#[derive(Clone)]
pub enum Scope {
    Metadata,
    Active {
        binding: Binding,
        cancel: Option<CancellationToken>,
        guard: Option<CancellationToken>,
    },
    Cleanup {
        uid: Uid,
    },
    StorageCleanup {
        uid: Uid,
        kind: crate::storage_wire::Cleanup,
        session: u64,
    },
    StorageActive {
        binding: Binding,
        cancel: CancellationToken,
        key: CancellationToken,
        client: CancellationToken,
        session: u64,
    },
    NetworkActive {
        binding: Binding,
        life: CancellationToken,
        key: CancellationToken,
        client: CancellationToken,
        session: u64,
    },
    NetworkCleanup {
        uid: Uid,
        session: u64,
    },
}
impl Scope {
    fn validate_session(&self, current: u64) -> Result<(), AppError> {
        if let Self::StorageActive { session, .. }
        | Self::StorageCleanup { session, .. }
        | Self::NetworkActive { session, .. }
        | Self::NetworkCleanup { session, .. } = self
        {
            if *session == 0 || *session != current {
                return Err(AppError::new(
                    "STALE_SESSION",
                    "원래 MCU 관리 세션이 종료됐습니다.",
                ));
            }
        }
        Ok(())
    }
    fn validate(
        &self,
        active: &ActiveCoordinator,
        uid: &Uid,
        ch: u8,
        op: u16,
        payload: &[u8],
    ) -> Result<(), AppError> {
        match self {
            Self::NetworkActive {
                binding,
                life,
                key,
                client,
                ..
            } if &binding.uid == uid
                && !life.is_cancelled()
                && !key.is_cancelled()
                && !client.is_cancelled()
                && crate::network::wire::allowed(ch, op, payload) =>
            {
                active.validate(binding)
            }
            Self::NetworkCleanup { uid: expected, .. }
                if expected == uid
                    && ch == 4
                    && ((op == 0x401 && payload == [0]) || (op == 0x403 && payload.is_empty())) =>
            {
                Ok(())
            }
            Self::StorageActive {
                binding,
                cancel,
                key,
                client,
                ..
            } if &binding.uid == uid
                && !cancel.is_cancelled()
                && !key.is_cancelled()
                && !client.is_cancelled() =>
            {
                active.validate(binding)
            }
            Self::StorageCleanup {
                uid: expected,
                kind,
                ..
            } if expected == uid && kind.matches(ch, op, payload) => Ok(()),
            Self::Metadata
                if (ch == 0
                    && ((op == 2 && payload.len() == 8)
                        || ([3, 4].contains(&op) && payload.is_empty())))
                    || (ch == 3 && op == 0x31e && payload.is_empty()) =>
            {
                Ok(())
            }
            Self::Active {
                binding,
                cancel,
                guard,
            } if &binding.uid == uid
                && !cancel.as_ref().is_some_and(|c| c.is_cancelled())
                && !guard.as_ref().is_some_and(|c| c.is_cancelled()) =>
            {
                active.validate(binding)
            }
            Self::Cleanup { uid: expected }
                if expected == uid
                    && ((ch == 1 && op == 0x107 && payload.is_empty())
                        || (ch == 1
                            && op == 0x102
                            && payload.len() == 2
                            && crate::input::Action::Key {
                                down: false,
                                usage: u16::from_le_bytes([payload[0], payload[1]]),
                            }
                            .packet()
                            .is_ok())
                        || (ch == 1
                            && op == 0x105
                            && payload.len() == 1
                            && (1..=5).contains(&payload[0]))) =>
            {
                Ok(())
            }
            _ => Err(AppError::new(
                "STALE_OPERATION",
                "채널/입력 소유권 변경으로 이전 요청을 취소했습니다.",
            )),
        }
    }
}
struct Request {
    scope: Scope,
    ch: u8,
    op: u16,
    payload: Vec<u8>,
    completion: Completion,
    session: u64,
    reply: oneshot::Sender<Result<Vec<u8>, AppError>>,
    permit: tokio::sync::OwnedSemaphorePermit,
}
enum Responder {
    Client(
        oneshot::Sender<Result<Vec<u8>, AppError>>,
        tokio::sync::OwnedSemaphorePermit,
    ),
    Status,
    Name,
}
#[derive(Default)]
struct State {
    mcu: Option<crate::diagnostics::McuStatus>,
    channel: Option<Channel>,
    session: u64,
    error: String,
    finished: bool,
    event_gaps: u64,
    crc_rejected: u64,
    fault_request: Option<(u8, u16, u32)>,
    os_error: Option<u32>,
}
#[derive(Clone)]
pub struct DeviceEvent {
    pub uid: Uid,
    pub session: u64,
    pub frame: Frame,
}
#[derive(Clone)]
struct EventBus {
    all: broadcast::Sender<DeviceEvent>,
    channels: [broadcast::Sender<DeviceEvent>; 6],
}
impl EventBus {
    fn new() -> Self {
        Self {
            all: broadcast::channel(256).0,
            // A complete 32KiB NCM credit window can contain >1000 tiny frames.
            // Keep that window bounded without letting its flood evict TTY events.
            channels: std::array::from_fn(|ch| {
                broadcast::channel(if ch == 4 { 2048 } else { 256 }).0
            }),
        }
    }
    fn send(&self, event: DeviceEvent) {
        if let Some(channel) = self
            .channels
            .get(usize::from(event.frame.channel))
            .filter(|tx| tx.receiver_count() > 0)
        {
            let _ = channel.send(event.clone());
        }
        let _ = self.all.send(event);
    }
}
pub struct DeviceHandle {
    wake: crate::serial_io::Wake,
    identity: u64,
    queues: [mpsc::Sender<Request>; 6],
    slots: [Arc<tokio::sync::Semaphore>; 6],
    urgent_slots: Arc<tokio::sync::Semaphore>,
    urgent: mpsc::Sender<Request>,
    state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
    worker: Mutex<Option<std::thread::JoinHandle<()>>>,
    pub fingerprint: String,
    events: EventBus,
}
impl DeviceHandle {
    fn spawn(
        path: String,
        fingerprint: String,
        active: Arc<ActiveCoordinator>,
        logs: Option<Arc<crate::logging::Journal>>,
    ) -> Arc<Self> {
        let (senders, receivers): (Vec<_>, Vec<_>) = (0..6).map(|_| mpsc::channel(32)).unzip();
        let (urgent, rx) = mpsc::channel(8);
        let state = Arc::new(Mutex::new(State::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let events = EventBus::new();
        let handle = Arc::new(Self {
            wake: crate::serial_io::Wake::new().expect("MCU wake event"),
            identity: NEXT_IDENTITY
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
                .expect("MCU worker identity exhausted"),
            queues: senders.try_into().unwrap_or_else(|_| unreachable!()),
            urgent,
            slots: std::array::from_fn(|_| Arc::new(tokio::sync::Semaphore::new(32))),
            urgent_slots: Arc::new(tokio::sync::Semaphore::new(8)),
            state: state.clone(),
            stop: stop.clone(),
            worker: Mutex::new(None),
            fingerprint,
            events: events.clone(),
        });
        let wake = handle.wake.clone();
        let worker = std::thread::Builder::new()
            .name(format!("MiniKMB {path}"))
            .stack_size(256 * 1024)
            .spawn(move || {
                let result = worker(
                    &path,
                    active,
                    state.clone(),
                    stop,
                    receivers.try_into().unwrap_or_else(|_| unreachable!()),
                    rx,
                    events,
                    wake,
                );
                let mut s = state.lock();
                if let (Some(logs), Err(e)) = (logs.as_ref(), &result) {
                    logs.emit(
                        crate::logging::Level::Warn,
                        crate::logging::Event::TransportFailed,
                        crate::logging::Fields {
                            uid: s.channel.as_ref().map(|c| c.uid.clone()),
                            session: (s.session != 0).then_some(s.session),
                            channel: s.fault_request.map(|r| r.0),
                            opcode: s.fault_request.map(|r| r.1),
                            sequence: s.fault_request.map(|r| r.2),
                            os_error: s.os_error,
                            code: Some(e.code),
                            success: Some(false),
                            uncertain: Some(e.uncertain),
                            ..Default::default()
                        },
                    );
                }
                s.finished = true;
                s.session = 0;
                if let Some(c) = s.channel.as_mut() {
                    c.connected = false;
                }
                if let Err(e) = result {
                    s.error = e.message;
                }
            });
        match worker {
            Ok(w) => *handle.worker.lock() = Some(w),
            Err(_) => {
                let mut s = handle.state.lock();
                s.finished = true;
                s.error = "관리 USB 워커를 시작할 수 없습니다.".into();
            }
        }
        handle
    }
    pub fn subscribe(&self) -> broadcast::Receiver<DeviceEvent> {
        self.events.all.subscribe()
    }
    pub fn subscribe_channel(
        &self,
        channel: u8,
    ) -> Result<broadcast::Receiver<DeviceEvent>, AppError> {
        self.events
            .channels
            .get(usize::from(channel))
            .map(|tx| tx.subscribe())
            .ok_or_else(|| AppError::new("INVALID_COMMAND", "이벤트 채널 범위를 확인하세요."))
    }
    pub fn session(&self) -> u64 {
        self.state.lock().session
    }
    pub fn identity(&self) -> u64 {
        self.identity
    }
    pub fn channel(&self) -> Option<Channel> {
        self.state.lock().channel.clone()
    }
    pub fn diagnostics(&self) -> crate::diagnostics::Transport {
        let s = self.state.lock();
        crate::diagnostics::Transport {
            session: s.session,
            event_gaps: s.event_gaps,
            crc_rejected: s.crc_rejected,
            faulted: !s.error.is_empty(),
            mcu: s.mcu.clone(),
        }
    }
    pub fn verified_name(&self, session: u64, name: &str) -> Result<(), AppError> {
        let mut state = self.state.lock();
        if state.session != session || state.finished {
            return Err(AppError::new(
                "STALE_SESSION",
                "이름 조회 뒤 MCU 세션이 바뀌었습니다.",
            ));
        }
        if let Some(c) = &mut state.channel {
            c.name = name.to_owned();
        }
        Ok(())
    }
    pub async fn request(
        &self,
        scope: Scope,
        ch: u8,
        op: u16,
        payload: Vec<u8>,
        completion: Completion,
    ) -> Result<Vec<u8>, AppError> {
        if ch > 5 || payload.len() > 1024 {
            return Err(AppError::new(
                "INVALID_COMMAND",
                "관리 명령 범위를 확인하세요.",
            ));
        }
        let session = {
            let s = self.state.lock();
            if s.finished || s.session == 0 {
                return Err(AppError::new(
                    "DEVICE_DISCONNECTED",
                    "MCU 연결이 끊겼습니다.",
                ));
            }
            scope.validate_session(s.session)?;
            s.session
        };
        let cleanup = matches!(
            scope,
            Scope::Cleanup { .. } | Scope::StorageCleanup { .. } | Scope::NetworkCleanup { .. }
        );
        let slots = if cleanup {
            self.urgent_slots.clone()
        } else {
            self.slots[ch as usize].clone()
        };
        let permit = slots
            .try_acquire_owned()
            .map_err(|_| AppError::new("QUEUE_FULL", "관리 명령 대기열이 찼습니다."))?;
        let (reply, rx) = oneshot::channel();
        let r = Request {
            scope,
            ch,
            op,
            payload,
            completion,
            session,
            reply,
            permit,
        };
        let queue = if cleanup {
            &self.urgent
        } else {
            &self.queues[ch as usize]
        };
        queue
            .try_send(r)
            .map_err(|_| AppError::new("QUEUE_FULL", "관리 명령 대기열이 찼거나 종료됐습니다."))?;
        self.wake.signal();
        rx.await.map_err(|_| {
            AppError::uncertain(
                "DEVICE_DISCONNECTED",
                "MCU 연결이 종료돼 명령 완료를 확인하지 못했습니다.",
            )
        })?
    }
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Release);
        self.wake.signal();
    }
    fn join(&self) {
        if let Some(w) = self.worker.lock().take() {
            let _ = w.join();
        }
    }
}
impl Drop for DeviceHandle {
    fn drop(&mut self) {
        self.stop();
    }
}
fn process(
    effects: Vec<Effect>,
    responders: &mut HashMap<u64, Responder>,
    state: &Mutex<State>,
    events: &EventBus,
) -> Result<(), AppError> {
    for effect in effects {
        match effect {
            Effect::Done(id, result) => {
                if let Some(r) = responders.remove(&id) {
                    match r {
                        Responder::Client(tx, _permit) => {
                            let _ = tx.send(result);
                        }
                        Responder::Status => match result {
                            Err(e) => return Err(e),
                            Ok(b) if b.len() == 64 => {
                                let mut s = state.lock();
                                s.mcu = Some(crate::diagnostics::McuStatus::parse(&b, now_ms())?);
                                if let Some(c) = s.channel.as_mut() {
                                    c.target_state = b[12];
                                    c.observed_at = now_ms();
                                }
                            }
                            _ => {
                                return Err(AppError::new(
                                    "INVALID_RESPONSE",
                                    "상태 응답을 확인할 수 없습니다.",
                                ))
                            }
                        },
                        Responder::Name => {
                            let mut s = state.lock();
                            if let Some(c) = s.channel.as_mut() {
                                c.name = result
                                    .ok()
                                    .and_then(|b| {
                                        if b.len() >= 4
                                            && b[2..4] == [0, 0]
                                            && b.len()
                                                == 4 + usize::from(u16::from_le_bytes([b[0], b[1]]))
                                            && b.len() <= 68
                                        {
                                            String::from_utf8(b[4..].to_vec()).ok()
                                        } else {
                                            None
                                        }
                                    })
                                    .unwrap_or_default();
                            }
                        }
                    }
                }
            }
            Effect::Event(frame) => {
                let s = state.lock();
                if let Some(c) = &s.channel {
                    events.send(DeviceEvent {
                        uid: c.uid.clone(),
                        session: s.session,
                        frame,
                    });
                }
            }
        }
    }
    Ok(())
}
#[derive(Default)]
struct Scheduler {
    high_burst: u8,
    low_next: usize,
}
impl Scheduler {
    fn choose(&mut self, ready: [bool; 6]) -> Option<usize> {
        let high = [1, 0, 2].into_iter().find(|i| ready[*i]);
        let low = (0..3)
            .map(|n| 3 + (self.low_next + n) % 3)
            .find(|i| ready[*i]);
        if let Some(ch) = low.filter(|_| self.high_burst >= 8 || high.is_none()) {
            self.low_next = (ch - 3 + 1) % 3;
            self.high_burst = 0;
            return Some(ch);
        }
        if high.is_some() {
            self.high_burst = (self.high_burst + 1).min(8);
        }
        high
    }
}
fn io_error() -> AppError {
    AppError::uncertain(
        "DEVICE_IO",
        "관리 USB 입출력이 실패했습니다. 자동 재전송하지 않습니다.",
    )
}
#[allow(clippy::too_many_arguments)]
fn worker(
    path: &str,
    active: Arc<ActiveCoordinator>,
    state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
    mut queues: [mpsc::Receiver<Request>; 6],
    mut urgent: mpsc::Receiver<Request>,
    events: EventBus,
    wake: crate::serial_io::Wake,
) -> Result<(), AppError> {
    let mut session = Session::open(path).map_err(|_| {
        AppError::new(
            "CONNECT_FAILED",
            "관리 포트 열기/HELLO 검증이 실패했습니다.",
        )
    })?;
    session.read_name();
    let uid = session.channel.uid.clone();
    let mut machine = Machine::new(session.session, session.sequence);
    {
        let mut s = state.lock();
        s.session = session.session;
        s.channel = Some(session.channel.clone());
    }
    let mut responders = HashMap::new();
    let mut waiting: [VecDeque<Request>; 6] = std::array::from_fn(|_| VecDeque::new());
    let mut cleanup = VecDeque::new();
    let mut id = 0u64;
    let clock = Instant::now();
    let mut last_status = Instant::now();
    let mut last_name = Instant::now();
    let mut writing = None;
    let mut scheduler = Scheduler::default();
    let result = (|| {
        while !stop.load(Ordering::Acquire) {
            let now = clock.elapsed().as_millis() as u64;
            let (bytes, completed) = session.port.poll().map_err(|e| {
                state.lock().os_error = e.raw_os_error().and_then(|n| n.try_into().ok());
                io_error()
            })?;
            if completed {
                if let Some((ch, sequence)) = writing.take() {
                    machine.written(ch, sequence, now);
                }
            }
            session.frames.extend(session.decoder.feed(&bytes));
            while let Some(frame) = session.frames.pop_front() {
                process(machine.feed(frame, now), &mut responders, &state, &events)?;
            }
            process(machine.tick(now), &mut responders, &state, &events)?;
            if machine.fatal.is_some() {
                return Err(machine.fatal.clone().unwrap());
            }
            // Queue storage remains bounded even while a channel is awaiting ACK.
            if cleanup.len() < 8 {
                if let Ok(r) = urgent.try_recv() {
                    cleanup.push_back(r);
                }
            }
            for ch in 0..6 {
                if waiting[ch].is_empty() {
                    if let Ok(r) = queues[ch].try_recv() {
                        waiting[ch].push_back(r);
                    }
                }
            }
            let urgent_ready = cleanup
                .front()
                .is_some_and(|r| machine.can_send(r.ch, r.op, r.completion));
            if urgent_ready && session.port.can_write() {
                let r = cleanup.pop_front().unwrap();
                send_request(
                    r,
                    &uid,
                    &active,
                    &mut session,
                    &mut machine,
                    &mut id,
                    &mut responders,
                    now,
                )?;
            } else if session.port.can_write() {
                // One frame boundary at a time; input/control are checked before bulk.
                let ready = std::array::from_fn(|ch| {
                    waiting[ch]
                        .front()
                        .is_some_and(|r| machine.can_send(r.ch, r.op, r.completion))
                });
                if let Some(ch) = scheduler.choose(ready) {
                    let r = waiting[ch].pop_front().unwrap();
                    send_request(
                        r,
                        &uid,
                        &active,
                        &mut session,
                        &mut machine,
                        &mut id,
                        &mut responders,
                        now,
                    )?;
                }
            }
            if last_status.elapsed() >= Duration::from_millis(500)
                && session.port.can_write()
                && machine.can_send(0, 4, Completion::Ack)
            {
                id = id.checked_add(1).ok_or_else(io_error)?;
                let frame = machine.prepare(id, 0, 4, Vec::new(), Completion::Ack, now)?;
                responders.insert(id, Responder::Status);
                session
                    .port
                    .start_write(&frame.encode().map_err(|_| io_error())?)
                    .map_err(|_| io_error())?;
                machine.writing(frame.channel, frame.sequence);
                last_status = Instant::now();
            }
            if last_name.elapsed() >= Duration::from_secs(3)
                && session.port.can_write()
                && waiting[3].is_empty()
                && queues[3].is_empty()
                && machine.can_send(3, 0x31e, Completion::Ack)
            {
                id = id.checked_add(1).ok_or_else(io_error)?;
                let frame = machine.prepare(id, 3, 0x31e, Vec::new(), Completion::Ack, now)?;
                responders.insert(id, Responder::Name);
                session
                    .port
                    .start_write(&frame.encode().map_err(|_| io_error())?)
                    .map_err(|_| io_error())?;
                machine.writing(frame.channel, frame.sequence);
                last_name = Instant::now();
            }
            if !session.port.can_write() && writing.is_none() {
                writing = machine.last_request.map(|(ch, _, sequence)| (ch, sequence));
            }
            {
                let mut s = state.lock();
                s.event_gaps = machine.event_gaps;
                s.crc_rejected = session.decoder.rejected;
            }
            let deadline = machine.next_deadline().unwrap_or(now + 500).min(now + 500);
            // Completion events and new queue/stop signals wake this wait. The
            // bound serves heartbeat/protocol maintenance, not I/O readiness.
            let mut wait = Duration::from_millis(deadline.saturating_sub(now));
            if session.port.can_write() {
                if machine.can_send(0, 4, Completion::Ack) {
                    wait =
                        wait.min(Duration::from_millis(500).saturating_sub(last_status.elapsed()));
                }
                if waiting[3].is_empty()
                    && queues[3].is_empty()
                    && machine.can_send(3, 0x31e, Completion::Ack)
                {
                    wait = wait.min(Duration::from_secs(3).saturating_sub(last_name.elapsed()));
                }
            }
            if session.port.can_write() && (urgent_ready || waiting.iter().any(|q| !q.is_empty())) {
                // Ready requests on blocked protocol channels still wait for RX.
                if cleanup
                    .front()
                    .is_some_and(|r| machine.can_send(r.ch, r.op, r.completion))
                    || waiting.iter().any(|q| {
                        q.front()
                            .is_some_and(|r| machine.can_send(r.ch, r.op, r.completion))
                    })
                {
                    wait = Duration::ZERO;
                }
            }
            session
                .port
                .wait(Some(&wake), wait)
                .map_err(|_| io_error())?;
            // A continuously signaled RX/TX event must not turn this worker
            // into an unbroken CPU owner. Yield without a fixed sleep delay.
            std::thread::yield_now();
        }
        Ok(())
    })();
    if result.is_err() {
        state.lock().fault_request = machine.fault_request.or(machine.last_request);
    }
    let _ = process(machine.disconnect(), &mut responders, &state, &events);
    for r in waiting.into_iter().flatten().chain(cleanup) {
        let _ = r.reply.send(Err(AppError::new(
            "DEVICE_DISCONNECTED",
            "MCU 워커를 종료했습니다.",
        )));
    }
    result
}
#[allow(clippy::too_many_arguments)]
fn send_request(
    r: Request,
    uid: &Uid,
    active: &ActiveCoordinator,
    session: &mut Session,
    machine: &mut Machine,
    id: &mut u64,
    responders: &mut HashMap<u64, Responder>,
    now: u64,
) -> Result<(), AppError> {
    if r.reply.is_closed() {
        return Ok(());
    }
    if r.session != session.session {
        let _ = r.reply.send(Err(AppError::new(
            "STALE_OPERATION",
            "MCU 세션이 변경됐습니다.",
        )));
        return Ok(());
    }
    if let Err(e) = r.scope.validate(active, uid, r.ch, r.op, &r.payload) {
        let _ = r.reply.send(Err(e));
        return Ok(());
    }
    *id = id.checked_add(1).ok_or_else(io_error)?;
    let frame = machine.prepare(*id, r.ch, r.op, r.payload, r.completion, now)?;
    responders.insert(*id, Responder::Client(r.reply, r.permit));
    session
        .port
        .start_write(&frame.encode().map_err(|_| io_error())?)
        .map_err(|_| io_error())?;
    machine.writing(frame.channel, frame.sequence);
    Ok(())
}

pub struct DeviceHub {
    logs: Option<Arc<crate::logging::Journal>>,
    active: Arc<ActiveCoordinator>,
    actors: RwLock<HashMap<String, Arc<DeviceHandle>>>,
    retired: Mutex<Vec<Arc<DeviceHandle>>>,
    nodes: RwLock<Vec<crate::topology::UsbNode>>,
    error: Mutex<String>,
    revision: AtomicU64,
}
impl DeviceHub {
    pub fn new(active: Arc<ActiveCoordinator>) -> Self {
        Self {
            logs: None,
            active,
            actors: Default::default(),
            retired: Default::default(),
            nodes: Default::default(),
            error: Default::default(),
            revision: AtomicU64::new(0),
        }
    }
    pub fn with_logger(active: Arc<ActiveCoordinator>, logs: Arc<crate::logging::Journal>) -> Self {
        Self {
            logs: Some(logs),
            ..Self::new(active)
        }
    }
    pub fn scan(&self) {
        use serialport::SerialPortType;
        let ports = match serialport::available_ports() {
            Ok(p) => p,
            Err(_) => {
                *self.error.lock() = "관리 USB 검색에 실패했습니다.".into();
                return;
            }
        };
        let allowed: Vec<_> = ports
            .into_iter()
            .filter_map(|p| match p.port_type {
                SerialPortType::UsbPort(u)
                    if u.vid == 0xcafe && matches!(u.pid, 0x4001 | 0x4002 | 0x4004) =>
                {
                    Some((
                        p.port_name,
                        format!(
                            "{}:{}:{}",
                            u.vid,
                            u.pid,
                            u.serial_number.unwrap_or_default()
                        ),
                    ))
                }
                _ => None,
            })
            .take(64)
            .collect();
        let mut actors = self.actors.write();
        let remove: Vec<_> = actors
            .iter()
            .filter(|(path, a)| {
                a.state.lock().finished
                    || !allowed
                        .iter()
                        .any(|(p, f)| p == *path && f == &a.fingerprint)
            })
            .map(|(p, _)| p.clone())
            .collect();
        for path in remove {
            if let Some(a) = actors.remove(&path) {
                a.stop();
                self.retired.lock().push(a);
            }
        }
        for (path, fingerprint) in allowed {
            actors.entry(path.clone()).or_insert_with(|| {
                DeviceHandle::spawn(path, fingerprint, self.active.clone(), self.logs.clone())
            });
        }
        drop(actors);
        self.retired.lock().retain(|a| {
            if a.state.lock().finished {
                a.join();
                false
            } else {
                true
            }
        });
        match crate::platform::usb_nodes() {
            Ok(n) => {
                *self.nodes.write() = n;
                self.error.lock().clear();
            }
            Err(_) => {
                self.nodes.write().clear();
                *self.error.lock() =
                    "USB 위치를 확인할 수 없어 자동 영상 연결을 중지했습니다.".into();
            }
        }
    }
    pub fn handle(&self, uid: &Uid) -> Result<Arc<DeviceHandle>, AppError> {
        let actors = self.actors.read();
        let mut found = actors.values().filter(|a| {
            let s = a.state.lock();
            s.session != 0
                && s.channel
                    .as_ref()
                    .is_some_and(|c| &c.uid == uid && c.connected)
        });
        let first = found.next().cloned().ok_or_else(|| {
            AppError::new("DEVICE_DISCONNECTED", "연결된 MCU를 찾을 수 없습니다.")
        })?;
        if found.next().is_some() {
            return Err(AppError::new(
                "DUPLICATE_UID",
                "중복 MCU UID를 구분할 수 없습니다.",
            ));
        }
        Ok(first)
    }
    pub fn inventory(&self) -> Inventory {
        let mut channels = Vec::new();
        let mut errors = Vec::new();
        for actor in self.actors.read().values() {
            let s = actor.state.lock();
            if let Some(c) = &s.channel {
                channels.push(c.clone());
            }
            if !s.error.is_empty() {
                errors.push(s.error.clone());
            }
        }
        let mut counts = HashMap::new();
        for c in &channels {
            *counts.entry(c.uid.clone()).or_insert(0) += 1;
        }
        crate::topology::pair(&self.nodes.read(), &mut channels);
        for c in &mut channels {
            if counts[&c.uid] > 1 {
                c.connected = false;
                c.pairing_error = "중복 MCU UID".into();
            }
        }
        channels.sort_by(|a, b| {
            a.board_id
                .cmp(&b.board_id)
                .then(a.usb_port.cmp(&b.usb_port))
        });
        let error = self.error.lock().clone();
        if !error.is_empty() {
            errors.push(error);
        }
        Inventory {
            channels,
            error: errors.join(" · "),
            observed_at: now_ms(),
            revision: self.revision.fetch_add(1, Ordering::Relaxed),
        }
    }
    pub fn stop(&self) {
        for actor in self.actors.read().values() {
            actor.stop();
        }
    }
    pub fn join(&self) {
        for actor in self.actors.read().values() {
            actor.join();
        }
        for a in self.retired.lock().drain(..) {
            a.stop();
            a.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn other_channel_flood_cannot_evict_serial_events() {
        let bus = EventBus::new();
        let mut serial = bus.channels[2].subscribe();
        let make = |channel| DeviceEvent {
            uid: Uid::from_bytes(&[1; 12]).unwrap(),
            session: 1,
            frame: Frame {
                kind: 4,
                channel,
                opcode: 0x280,
                session: 1,
                sequence: 1,
                payload: vec![42],
            },
        };
        bus.send(make(2));
        for _ in 0..600 {
            bus.send(make(4));
        }
        assert_eq!(
            serial
                .try_recv()
                .unwrap_or_else(|_| panic!("serial event lost"))
                .frame
                .payload,
            vec![42]
        );
        assert!(matches!(
            serial.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));
    }
    #[tokio::test]
    async fn accepted_request_bound_includes_dequeued_and_inflight_work() {
        use futures_util::FutureExt;
        let (senders, receivers): (Vec<_>, Vec<_>) = (0..6).map(|_| mpsc::channel(32)).unzip();
        let mut receivers: [mpsc::Receiver<Request>; 6] =
            receivers.try_into().unwrap_or_else(|_| unreachable!());
        let (urgent, _) = mpsc::channel(8);
        let events = EventBus::new();
        let handle = DeviceHandle {
            wake: crate::serial_io::Wake::new().unwrap(),
            identity: 1,
            queues: senders.try_into().unwrap_or_else(|_| unreachable!()),
            slots: std::array::from_fn(|_| Arc::new(tokio::sync::Semaphore::new(32))),
            urgent_slots: Arc::new(tokio::sync::Semaphore::new(8)),
            urgent,
            state: Arc::new(Mutex::new(State {
                session: 42,
                ..Default::default()
            })),
            stop: Default::default(),
            worker: Default::default(),
            fingerprint: String::new(),
            events,
        };
        let mut in_flight = Vec::new();
        for _ in 0..32 {
            assert!(handle
                .request(Scope::Metadata, 0, 4, Vec::new(), Completion::Ack)
                .now_or_never()
                .is_none());
            in_flight.push(receivers[0].try_recv().unwrap());
        }
        assert!(receivers[0].is_empty());
        assert_eq!(
            handle
                .request(Scope::Metadata, 0, 4, Vec::new(), Completion::Ack)
                .await
                .err()
                .unwrap()
                .code,
            "QUEUE_FULL"
        );
        in_flight.pop();
        assert!(handle
            .request(Scope::Metadata, 0, 4, Vec::new(), Completion::Ack)
            .now_or_never()
            .is_none());
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "Read-only hardware test; MINIKMB_WORKER_TEST_UIDS (two exact UIDs) and MINIKMB_WORKER_TEST_OUTPUT required; close Electron first"]
    async fn hardware_independent_mcu_workers() {
        let expected: Vec<_> = std::env::var("MINIKMB_WORKER_TEST_UIDS")
            .expect("exact UIDs required")
            .split(',')
            .map(|s| Uid::try_from(s.to_owned()).unwrap())
            .collect();
        assert_eq!(expected.len(), 2);
        let output = std::path::PathBuf::from(
            std::env::var("MINIKMB_WORKER_TEST_OUTPUT").expect("artifact directory required"),
        );
        assert!(output.is_dir());
        let hub = DeviceHub::new(Arc::new(ActiveCoordinator::default()));
        hub.scan();
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            if expected.iter().all(|uid| hub.handle(uid).is_ok()) {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "exact MCU pair missing: {}",
                hub.inventory().error
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let a = hub.handle(&expected[0]).unwrap();
        let b = hub.handle(&expected[1]).unwrap();
        let initial = [a.session(), b.session()];
        let exercise = |handle: Arc<DeviceHandle>| async move {
            let start = Instant::now();
            for n in 0..32u64 {
                let cookie = n.to_le_bytes();
                let result = handle
                    .request(Scope::Metadata, 0, 2, cookie.to_vec(), Completion::Ack)
                    .await
                    .unwrap();
                assert!(result.len() >= 8);
                assert_eq!(&result[..8], cookie);
            }
            start.elapsed().as_millis()
        };
        let elapsed = tokio::time::timeout(Duration::from_secs(8), async {
            tokio::join!(exercise(a.clone()), exercise(b.clone()))
        })
        .await
        .unwrap();
        let before = hub.inventory();
        tokio::time::sleep(Duration::from_secs(2)).await;
        hub.scan();
        let after = hub.inventory();
        assert_eq!(
            [a.session(), b.session()],
            initial,
            "rescan must not reopen working sessions"
        );
        for uid in &expected {
            let old = before.channels.iter().find(|c| &c.uid == uid).unwrap();
            let new = after.channels.iter().find(|c| &c.uid == uid).unwrap();
            assert!(new.connected && new.observed_at > old.observed_at);
            assert!(new.capture_id.is_some());
        }
        let states:Vec<_>=[a.clone(),b.clone()].iter().map(|h|{let s=h.state.lock();serde_json::json!({"uid":s.channel.as_ref().unwrap().uid,"event_gaps":s.event_gaps,"crc_rejected":s.crc_rejected})}).collect();
        hub.stop();
        hub.join();
        assert!(a.state.lock().finished && b.state.lock().finished);
        let evidence = serde_json::json!({"inventory":after,"ping_count_per_mcu":32,"parallel_ping_elapsed_ms":[elapsed.0,elapsed.1],"sessions_preserved_on_rescan":true,"heartbeat_continued":true,"workers_joined":true,"transport":states,"hid_sent":false,"sd_writes":false});
        std::fs::write(
            output.join("native-mcu-worker-readonly-validation.json"),
            serde_json::to_vec_pretty(&evidence).unwrap(),
        )
        .unwrap();
        println!("{evidence}");
    }
    #[test]
    fn input_has_priority_and_busy_input_does_not_starve_bulk_forever() {
        let mut s = Scheduler::default();
        let ready = [true; 6];
        for _ in 0..8 {
            assert_eq!(s.choose(ready), Some(1));
        }
        assert_eq!(s.choose(ready), Some(3));
        for _ in 0..8 {
            assert_eq!(s.choose(ready), Some(1));
        }
        assert_eq!(s.choose(ready), Some(4));
        for _ in 0..8 {
            assert_eq!(s.choose(ready), Some(1));
        }
        assert_eq!(s.choose(ready), Some(5));
        assert_eq!(
            Scheduler::default().choose([false, false, false, false, true, false]),
            Some(4)
        );
    }
    #[test]
    fn metadata_and_cleanup_scopes_cannot_send_arbitrary_input() {
        let active = ActiveCoordinator::default();
        let uid = Uid::from_bytes(&[1; 12]).unwrap();
        assert!(Scope::Metadata.validate(&active, &uid, 0, 4, &[]).is_ok());
        assert!(Scope::Metadata
            .validate(&active, &uid, 1, 0x101, &[4, 0])
            .is_err());
        let cleanup = Scope::Cleanup { uid: uid.clone() };
        assert!(cleanup.validate(&active, &uid, 1, 0x107, &[]).is_ok());
        assert!(cleanup.validate(&active, &uid, 1, 0x101, &[4, 0]).is_err());
        assert!(cleanup
            .validate(&active, &Uid::from_bytes(&[2; 12]).unwrap(), 1, 0x107, &[])
            .is_err());
    }
    #[test]
    fn queued_storage_checks_key_client_cancel_and_original_generation() {
        let active = ActiveCoordinator::default();
        let uid = Uid::from_bytes(&[1; 12]).unwrap();
        active.finish(Some(uid.clone()));
        for which in 0..3 {
            let cancel = CancellationToken::new();
            let key = CancellationToken::new();
            let client = CancellationToken::new();
            let scope = Scope::StorageActive {
                binding: active.bind().unwrap(),
                cancel: cancel.clone(),
                key: key.clone(),
                client: client.clone(),
                session: 7,
            };
            assert!(scope.validate(&active, &uid, 3, 0x315, &[]).is_ok());
            assert!(scope.validate_session(7).is_ok());
            assert!(scope.validate_session(8).is_err());
            [&cancel, &key, &client][which].cancel();
            assert!(scope.validate(&active, &uid, 3, 0x315, &[]).is_err());
        }
        let scope = Scope::StorageActive {
            binding: active.bind().unwrap(),
            cancel: Default::default(),
            key: Default::default(),
            client: Default::default(),
            session: 7,
        };
        active.begin().unwrap();
        active.finish(Some(uid.clone()));
        assert!(scope.validate(&active, &uid, 3, 0x315, &[]).is_err());
        let cleanup = Scope::StorageCleanup {
            uid,
            kind: crate::storage_wire::Cleanup::Disk(77),
            session: 7,
        };
        assert!(cleanup.validate_session(7).is_ok());
        assert!(cleanup.validate_session(8).is_err());
    }
    #[test]
    fn network_commands_and_cleanup_pin_session_and_cannot_send_hid() {
        let active = ActiveCoordinator::default();
        let uid = Uid::from_bytes(&[1; 12]).unwrap();
        active.finish(Some(uid.clone()));
        for which in 0..3 {
            let life = CancellationToken::new();
            let key = CancellationToken::new();
            let client = CancellationToken::new();
            let scope = Scope::NetworkActive {
                binding: active.bind().unwrap(),
                life: life.clone(),
                key: key.clone(),
                client: client.clone(),
                session: 7,
            };
            assert!(scope.validate(&active, &uid, 4, 0x401, &[1]).is_ok());
            assert!(scope
                .validate(&active, &uid, 0, 6, &crate::network::framing::credit(32768))
                .is_ok());
            assert!(scope.validate(&active, &uid, 1, 0x101, &[4, 0]).is_err());
            assert!(scope.validate_session(8).is_err());
            assert!(scope.validate_session(7).is_ok());
            [&life, &key, &client][which].cancel();
            assert!(scope.validate(&active, &uid, 4, 0x401, &[1]).is_err());
        }
        let scope = Scope::NetworkActive {
            binding: active.bind().unwrap(),
            life: Default::default(),
            key: Default::default(),
            client: Default::default(),
            session: 7,
        };
        active.begin().unwrap();
        active.finish(Some(uid.clone()));
        assert!(scope.validate(&active, &uid, 4, 0x403, &[]).is_err());
        let cleanup = Scope::NetworkCleanup {
            uid: uid.clone(),
            session: 7,
        };
        assert!(cleanup.validate(&active, &uid, 4, 0x401, &[0]).is_ok());
        assert!(cleanup.validate(&active, &uid, 4, 0x403, &[]).is_ok());
        assert!(cleanup.validate(&active, &uid, 4, 0x401, &[1]).is_err());
        assert!(cleanup.validate(&active, &uid, 1, 0x107, &[]).is_err());
        assert!(cleanup.validate_session(8).is_err());
        assert!(Scope::Cleanup { uid: uid.clone() }
            .validate(&active, &uid, 4, 0x401, &[0])
            .is_err());
    }
    #[test]
    fn network_event_bus_holds_one_full_credit_window_of_small_fragments() {
        let bus = EventBus::new();
        let mut network = bus.channels[4].subscribe();
        let mut serial = bus.channels[2].subscribe();
        let uid = Uid::from_bytes(&[1; 12]).unwrap();
        let count = 32768 / 17;
        for sequence in 1..=count {
            bus.send(DeviceEvent {
                uid: uid.clone(),
                session: 7,
                frame: Frame {
                    kind: 4,
                    channel: 4,
                    opcode: 0x480,
                    session: 7,
                    sequence,
                    payload: vec![0; 17],
                },
            });
        }
        bus.send(DeviceEvent {
            uid,
            session: 7,
            frame: Frame {
                kind: 4,
                channel: 2,
                opcode: 0x280,
                session: 7,
                sequence: 1,
                payload: vec![99],
            },
        });
        for _ in 0..count {
            assert!(network.try_recv().is_ok());
        }
        assert_eq!(serial.try_recv().unwrap().frame.payload, [99]);
    }
}
