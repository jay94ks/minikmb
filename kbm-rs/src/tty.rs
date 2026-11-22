//! Bounded target CDC bridge. This never opens the target's host COM port.
use crate::{
    active::{ActiveCoordinator, Binding},
    actor::{DeviceEvent, DeviceHub, Scope},
    input_sessions::Principal,
    model::{AppError, Uid},
    transport::Completion,
};
use futures_util::future::BoxFuture;
use parking_lot::Mutex;
use serde::Serialize;
use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
    time::Duration,
};
use tokio::sync::{broadcast, Semaphore};
use tokio_util::sync::CancellationToken;

pub const CAPACITY: usize = 65536;
const WINDOW: u64 = 32768;
const RENEW: u64 = 16384;
pub trait Wire: Send + Sync {
    fn live(&self) -> bool;
    fn send(
        &self,
        binding: Binding,
        owner: Principal,
        life: CancellationToken,
        channel: u8,
        opcode: u16,
        payload: Vec<u8>,
    ) -> BoxFuture<'static, Result<Vec<u8>, AppError>>;
}
pub struct Connection {
    pub session: u64,
    pub wire: Arc<dyn Wire>,
    pub events: broadcast::Receiver<DeviceEvent>,
}
pub trait Backend: Send + Sync {
    fn connect(&self, binding: &Binding) -> Result<Connection, AppError>;
}
pub struct NativeBackend {
    pub devices: Arc<DeviceHub>,
}
struct NativeWire {
    handle: Arc<crate::actor::DeviceHandle>,
    session: u64,
}
impl Wire for NativeWire {
    fn live(&self) -> bool {
        self.handle.session() == self.session && self.session != 0
    }
    fn send(
        &self,
        binding: Binding,
        owner: Principal,
        life: CancellationToken,
        channel: u8,
        opcode: u16,
        payload: Vec<u8>,
    ) -> BoxFuture<'static, Result<Vec<u8>, AppError>> {
        let handle = self.handle.clone();
        let session = self.session;
        Box::pin(async move {
            check_owner(&owner)?;
            if handle.session() != session {
                return Err(disconnected());
            }
            handle
                .request(
                    Scope::Active {
                        binding,
                        cancel: Some(life),
                        guard: Some(owner.ended),
                    },
                    channel,
                    opcode,
                    payload,
                    Completion::Ack,
                )
                .await
        })
    }
}
impl Backend for NativeBackend {
    fn connect(&self, binding: &Binding) -> Result<Connection, AppError> {
        let handle = self.devices.handle(&binding.uid)?;
        if handle.channel().is_none_or(|c| c.features & 2 == 0) {
            return Err(AppError::new(
                "UNSUPPORTED",
                "현재 펌웨어는 타깃 시리얼을 지원하지 않습니다.",
            ));
        }
        let session = handle.session();
        let events = handle.subscribe_channel(2)?;
        Ok(Connection {
            session,
            events,
            wire: Arc::new(NativeWire { handle, session }),
        })
    }
}
fn disconnected() -> AppError {
    AppError::new("DEVICE_DISCONNECTED", "시리얼 관리 연결이 종료됐습니다.")
}
fn check_owner(p: &Principal) -> Result<(), AppError> {
    if p.cancelled.is_cancelled() || p.ended.is_cancelled() {
        Err(AppError::new(
            "UNAUTHORIZED",
            "시리얼 제어 연결이 종료됐습니다.",
        ))
    } else {
        Ok(())
    }
}
#[derive(Clone, Default, Serialize, schemars::JsonSchema)]
pub struct LineStatus {
    pub target_open: bool,
    pub baud: u32,
    pub stop_bits: u8,
    pub parity: u8,
    pub data_bits: u8,
    pub tx_queued: u32,
    pub rx_queued: u32,
    pub tx_discarded: u32,
    pub rx_discarded: u32,
}
impl LineStatus {
    fn decode(b: &[u8], stats: &[u8]) -> Result<Self, AppError> {
        if b.len() != 16 || stats.len() != 8 || b[0] > 1 {
            return Err(AppError::new(
                "INVALID_RESPONSE",
                "타깃 시리얼 상태 길이/형식이 다릅니다.",
            ));
        }
        let u32at = |v: &[u8], n| u32::from_le_bytes(v[n..n + 4].try_into().unwrap());
        Ok(Self {
            target_open: b[0] != 0,
            stop_bits: b[1],
            parity: b[2],
            data_bits: b[3],
            baud: u32at(b, 4),
            tx_queued: u32at(b, 8),
            rx_queued: u32at(b, 12),
            tx_discarded: u32at(stats, 0),
            rx_discarded: u32at(stats, 4),
        })
    }
}
#[derive(Clone, Serialize, schemars::JsonSchema)]
pub struct Status {
    pub uid: Uid,
    pub generation: u64,
    pub receiving: bool,
    pub stream_id: String,
    pub bytes_received: u64,
    pub bytes_retained: usize,
    pub bytes_overwritten: u64,
    pub bytes_accepted: u64,
    pub capacity: usize,
    pub error: String,
    #[serde(flatten)]
    pub line: LineStatus,
}
#[derive(Serialize, schemars::JsonSchema)]
pub struct ReadResult {
    pub uid: Uid,
    pub generation: u64,
    pub stream_id: String,
    pub bytes: Vec<u8>,
    pub cursor: u64,
    pub end_cursor: u64,
    pub skipped: u64,
}
#[derive(Serialize, schemars::JsonSchema)]
pub struct WriteResult {
    pub uid: Uid,
    pub generation: u64,
    pub accepted: usize,
    pub total: usize,
    pub complete: bool,
    pub error: Option<AppError>,
}
struct Receiving {
    binding: Binding,
    owner: Principal,
    life: CancellationToken,
}
#[derive(Default)]
struct State {
    receiving: Option<Receiving>,
    visible: Option<Binding>,
    ring: VecDeque<u8>,
    retained_end: u64,
    last_sequence: u32,
    received: u64,
    accepted: u64,
    overwritten: u64,
    granted: u64,
    epoch: u64,
    line: LineStatus,
    error: String,
    broken: bool,
}
struct Context {
    uid: Uid,
    session: u64,
    wire: Arc<dyn Wire>,
    state: Mutex<State>,
    stop: CancellationToken,
    credit_gate: tokio::sync::Mutex<()>,
    write_gate: tokio::sync::Mutex<()>,
    writes: Arc<Semaphore>,
    worker: Mutex<Option<tokio::task::JoinHandle<()>>>,
}
impl Context {
    fn close(&self) {
        if let Some(r) = self.state.lock().receiving.take() {
            r.life.cancel();
        }
    }
    fn fail(&self, error: &str, broken: bool) {
        let mut s = self.state.lock();
        if let Some(r) = s.receiving.take() {
            r.life.cancel();
        }
        s.error = error.into();
        s.broken |= broken;
    }
    fn credit_failed(&self, e: &AppError) {
        if ![
            "STALE_OPERATION",
            "CHANNEL_SWITCHING",
            "UNAUTHORIZED",
            "TTY_CLOSED",
        ]
        .contains(&e.code)
        {
            self.fail(&e.message, true);
        }
    }
    fn status(&self, b: &Binding) -> Status {
        let s = self.state.lock();
        Status {
            uid: b.uid.clone(),
            generation: b.generation,
            receiving: s.receiving.as_ref().is_some_and(|r| {
                &r.binding == b && !r.life.is_cancelled() && !r.owner.ended.is_cancelled()
            }),
            stream_id: format!("{}-{}", self.session, s.epoch),
            bytes_received: s.received,
            bytes_retained: s.ring.len(),
            bytes_overwritten: s.overwritten,
            bytes_accepted: s.accepted,
            capacity: CAPACITY,
            error: s.error.clone(),
            line: s.line.clone(),
        }
    }
    fn receive(&self, event: DeviceEvent) {
        if event.uid != self.uid
            || event.session != self.session
            || event.frame.session != self.session
            || event.frame.kind != 4
            || event.frame.channel != 2
            || event.frame.opcode != 0x280
        {
            return;
        }
        if event.frame.payload.is_empty() || event.frame.payload.len() > 256 {
            self.fail("시리얼 수신 프레임 길이가 잘못됐습니다.", true);
            return;
        }
        let mut s = self.state.lock();
        if s.last_sequence.checked_add(1) != Some(event.frame.sequence) {
            drop(s);
            self.fail(
                "시리얼 수신 순서가 유실됐습니다. 관리 USB 재연결이 필요합니다.",
                true,
            );
            return;
        }
        s.last_sequence = event.frame.sequence;
        let Some(next) = s.received.checked_add(event.frame.payload.len() as u64) else {
            drop(s);
            self.fail("시리얼 수신 카운터 범위를 초과했습니다.", true);
            return;
        };
        if next > s.granted {
            drop(s);
            self.fail("시리얼 수신이 허용한 credit을 초과했습니다.", true);
            return;
        }
        s.received = next;
        // Drain already granted bytes even after close, without renewing credit.
        if s.receiving.is_none() {
            return;
        }
        s.retained_end = next;
        for byte in event.frame.payload {
            if s.ring.len() == CAPACITY {
                s.ring.pop_front();
                s.overwritten += 1;
            }
            s.ring.push_back(byte);
        }
    }
    async fn credit(&self, active: &ActiveCoordinator, force: bool) -> Result<(), AppError> {
        let _gate = self.credit_gate.lock().await;
        let request = {
            let mut s = self.state.lock();
            let Some(r) = &s.receiving else {
                return Ok(());
            };
            active.validate(&r.binding)?;
            check_owner(&r.owner)?;
            if r.life.is_cancelled() {
                return Err(AppError::new("TTY_CLOSED", "시리얼 수신이 중지됐습니다."));
            }
            if !force && s.granted.saturating_sub(s.received) > RENEW {
                return Ok(());
            }
            let limit = s
                .received
                .checked_add(WINDOW)
                .ok_or_else(|| AppError::new("RANGE", "시리얼 credit 범위를 초과했습니다."))?
                .max(s.granted);
            let args = (r.binding.clone(), r.owner.clone(), r.life.clone(), limit);
            // Events can precede the ACK; admission uses the issued upper bound.
            s.granted = limit;
            args
        };
        let (b, owner, life, limit) = request;
        let mut payload = vec![2, 0, 0, 0];
        payload.extend_from_slice(&limit.to_le_bytes());
        let result = self.wire.send(b, owner, life, 0, 6, payload).await?;
        if result.len() != 16
            || u64::from_le_bytes(result[0..8].try_into().unwrap()) > limit
            || u64::from_le_bytes(result[8..16].try_into().unwrap()) != limit
        {
            return Err(AppError::new(
                "INVALID_RESPONSE",
                "시리얼 credit 수락 값을 확인하지 못했습니다.",
            ));
        }
        Ok(())
    }
}
pub struct TtyManager {
    active: Arc<ActiveCoordinator>,
    backend: Arc<dyn Backend>,
    contexts: Mutex<HashMap<Uid, Arc<Context>>>,
}
impl TtyManager {
    pub fn new(active: Arc<ActiveCoordinator>, backend: Arc<dyn Backend>) -> Arc<Self> {
        Arc::new(Self {
            active,
            backend,
            contexts: Mutex::new(HashMap::new()),
        })
    }
    fn context(&self, b: &Binding) -> Result<Arc<Context>, AppError> {
        self.active.validate(b)?;
        let connection = self.backend.connect(b)?;
        let mut map = self.contexts.lock();
        if let Some(ctx) = map.get(&b.uid) {
            if ctx.session == connection.session && ctx.wire.live() {
                return Ok(ctx.clone());
            }
            ctx.stop.cancel();
        }
        // Retire disconnected entries before enforcing the retained-context bound.
        map.retain(|_, ctx| {
            if ctx.wire.live() {
                true
            } else {
                ctx.stop.cancel();
                false
            }
        });
        if map.len() >= 64 && !map.contains_key(&b.uid) {
            return Err(AppError::new(
                "QUEUE_FULL",
                "시리얼 채널 한도64개를 초과했습니다.",
            ));
        }
        let ctx = Arc::new(Context {
            uid: b.uid.clone(),
            session: connection.session,
            wire: connection.wire,
            state: Mutex::new(State {
                ring: VecDeque::with_capacity(CAPACITY),
                ..Default::default()
            }),
            stop: CancellationToken::new(),
            credit_gate: Default::default(),
            write_gate: Default::default(),
            writes: Arc::new(Semaphore::new(4)),
            worker: Default::default(),
        });
        let c = ctx.clone();
        let active = self.active.clone();
        let mut events = connection.events;
        *ctx.worker.lock() = Some(tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_millis(50));
            loop {
                tokio::select! {
                    biased;
                    _ = c.stop.cancelled() => break,
                    event = events.recv() => match event {
                        Ok(e) => c.receive(e),
                        Err(broadcast::error::RecvError::Lagged(_)) => { c.fail("시리얼 이벤트가 유실됐습니다. 관리 USB 재연결이 필요합니다.",true); break; },
                        Err(_) => { c.fail("시리얼 관리 연결이 종료됐습니다.",true); break; }
                    },
                    _ = tick.tick() => {
                        if !c.wire.live() { c.fail("시리얼 관리 연결이 종료됐습니다.",true); break; }
                        let valid = { let s=c.state.lock(); s.receiving.as_ref().is_some_and(|r| active.validate(&r.binding).is_ok() && check_owner(&r.owner).is_ok() && !r.life.is_cancelled()) };
                        if !valid { c.close(); continue; }
                        if let Err(e) = c.credit(&active,false).await { c.credit_failed(&e); }
                    }
                }
            }
        }));
        map.insert(b.uid.clone(), ctx.clone());
        Ok(ctx)
    }
    pub async fn open(&self, b: Binding, owner: Principal) -> Result<Status, AppError> {
        check_owner(&owner)?;
        let ctx = self.context(&b)?;
        {
            let mut s = ctx.state.lock();
            if s.broken {
                return Err(AppError::new(
                    "TTY_STREAM_GAP",
                    "시리얼 세션을 다시 사용할 수 없습니다. 관리 USB를 재연결하세요.",
                ));
            }
            if s.receiving.as_ref().is_some_and(|r| {
                r.binding == b && !r.life.is_cancelled() && check_owner(&r.owner).is_ok()
            }) {
                return Ok(ctx.status_after_unlock(s, &b));
            }
            if let Some(r) = s.receiving.take() {
                r.life.cancel();
            }
            s.epoch += 1;
            s.ring.clear();
            s.retained_end = s.received;
            s.visible = Some(b.clone());
            s.error.clear();
            s.receiving = Some(Receiving {
                binding: b.clone(),
                life: owner.cancelled.child_token(),
                owner,
            });
        }
        if let Err(e) = ctx.credit(&self.active, true).await {
            ctx.credit_failed(&e);
            return Err(e);
        }
        self.active.validate(&b)?;
        Ok(ctx.status(&b))
    }
    pub fn close(&self, b: &Binding) -> Result<(), AppError> {
        self.active.validate(b)?;
        if let Some(c) = self.contexts.lock().get(&b.uid) {
            c.close();
        }
        Ok(())
    }
    pub fn close_all(&self) {
        for c in self.contexts.lock().values() {
            c.close();
        }
    }
    pub fn cached(&self, b: &Binding) -> Option<Status> {
        self.contexts.lock().get(&b.uid).map(|c| c.status(b))
    }
    pub async fn status(&self, b: Binding, owner: Principal) -> Result<Status, AppError> {
        let ctx = self.context(&b)?;
        check_owner(&owner)?;
        let life = owner.cancelled.child_token();
        let line = ctx
            .wire
            .send(b.clone(), owner.clone(), life.clone(), 2, 0x202, vec![])
            .await?;
        let stats = ctx
            .wire
            .send(b.clone(), owner, life, 2, 0x203, vec![])
            .await?;
        let decoded = LineStatus::decode(&line, &stats)?;
        self.active.validate(&b)?;
        ctx.state.lock().line = decoded;
        Ok(ctx.status(&b))
    }
    pub fn read(
        &self,
        b: &Binding,
        stream: Option<&str>,
        cursor: Option<u64>,
        max: usize,
    ) -> Result<ReadResult, AppError> {
        self.active.validate(b)?;
        if !(1..=4096).contains(&max) || (cursor.is_some() && stream.is_none()) {
            return Err(AppError::new(
                "INVALID_ARGUMENTS",
                "시리얼 read는 1~4096바이트이며 cursor에는 stream_id가 필요합니다.",
            ));
        }
        let map = self.contexts.lock();
        let ctx = map
            .get(&b.uid)
            .ok_or_else(|| AppError::new("TTY_CLOSED", "먼저 시리얼 수신을 시작하세요."))?;
        let s = ctx.state.lock();
        let stream_id = format!("{}-{}", ctx.session, s.epoch);
        if s.visible.as_ref() != Some(b)
            || !ctx.wire.live()
            || s.epoch == 0
            || stream.is_some_and(|id| id != stream_id)
        {
            return Err(AppError::new(
                "STALE_STREAM",
                "시리얼 수신 세션이 변경됐습니다.",
            ));
        }
        let begin = s.retained_end - s.ring.len() as u64;
        let requested = cursor.unwrap_or(begin);
        if requested > s.retained_end {
            return Err(AppError::new(
                "INVALID_ARGUMENTS",
                "시리얼 cursor가 수신 범위를 벗어났습니다.",
            ));
        }
        let start = requested.max(begin);
        let bytes: Vec<_> = s
            .ring
            .iter()
            .skip((start - begin) as usize)
            .take(max)
            .copied()
            .collect();
        Ok(ReadResult {
            uid: b.uid.clone(),
            generation: b.generation,
            stream_id,
            cursor: start + bytes.len() as u64,
            end_cursor: s.retained_end,
            skipped: start - requested,
            bytes,
        })
    }
    pub async fn write(
        self: &Arc<Self>,
        b: Binding,
        owner: Principal,
        bytes: Vec<u8>,
    ) -> Result<WriteResult, AppError> {
        if bytes.is_empty() || bytes.len() > 65536 {
            return Err(AppError::new(
                "INVALID_ARGUMENTS",
                "시리얼 송신은 1~65536바이트입니다.",
            ));
        }
        check_owner(&owner)?;
        let ctx = self.context(&b)?;
        let permit = ctx
            .writes
            .clone()
            .try_acquire_owned()
            .map_err(|_| AppError::new("QUEUE_FULL", "시리얼 송신 대기열이 찼습니다."))?;
        let active = self.active.clone();
        // Retain ordered ownership and a bounded permit when the requester drops.
        tokio::spawn(async move {
            let _permit = permit;
            let _gate = ctx.write_gate.lock().await;
            let life = owner.cancelled.child_token();
            let mut accepted = 0;
            let mut error = None;
            for chunk in bytes.chunks(256) {
                let result = async {
                    active.validate(&b)?;
                    check_owner(&owner)?;
                    let ack = ctx
                        .wire
                        .send(
                            b.clone(),
                            owner.clone(),
                            life.clone(),
                            2,
                            0x201,
                            chunk.to_vec(),
                        )
                        .await?;
                    if ack.len() != 2
                        || usize::from(u16::from_le_bytes(ack.try_into().unwrap())) != chunk.len()
                    {
                        return Err(AppError::uncertain(
                            "INVALID_RESPONSE",
                            "시리얼 수락 길이가 다릅니다. 자동 재전송하지 않았습니다.",
                        ));
                    }
                    Ok(())
                }
                .await;
                match result {
                    Ok(()) => {
                        accepted += chunk.len();
                        ctx.state.lock().accepted += chunk.len() as u64;
                    }
                    Err(e) => {
                        error = Some(e);
                        break;
                    }
                }
            }
            Ok(WriteResult {
                uid: b.uid,
                generation: b.generation,
                accepted,
                total: bytes.len(),
                complete: accepted == bytes.len(),
                error,
            })
        })
        .await
        .map_err(|_| AppError::uncertain("TTY_WRITE_FAILED", "시리얼 송신 작업이 종료됐습니다."))?
    }
    pub async fn shutdown(&self) {
        let contexts: Vec<_> = self.contexts.lock().drain().map(|(_, c)| c).collect();
        for c in &contexts {
            c.close();
            c.stop.cancel();
        }
        for c in contexts {
            let worker = c.worker.lock().take();
            if let Some(w) = worker {
                let _ = w.await;
            }
        }
    }
}
impl Context {
    fn status_after_unlock(
        &self,
        guard: parking_lot::MutexGuard<'_, State>,
        b: &Binding,
    ) -> Status {
        drop(guard);
        self.status(b)
    }
}
impl Drop for TtyManager {
    fn drop(&mut self) {
        for c in self.contexts.lock().values() {
            c.close();
            c.stop.cancel();
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::protocol::Frame;
    use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
    type Calls = Vec<(Binding, u8, u16, Vec<u8>)>;
    struct Fake {
        calls: Mutex<Calls>,
        sent: AtomicU64,
        writes: AtomicUsize,
        fail_at: AtomicUsize,
        live: AtomicBool,
    }
    struct FakeWire(Arc<Fake>);
    impl Wire for FakeWire {
        fn live(&self) -> bool {
            self.0.live.load(Ordering::Acquire)
        }
        fn send(
            &self,
            b: Binding,
            p: Principal,
            life: CancellationToken,
            ch: u8,
            op: u16,
            payload: Vec<u8>,
        ) -> BoxFuture<'static, Result<Vec<u8>, AppError>> {
            let state = self.0.clone();
            Box::pin(async move {
                check_owner(&p)?;
                if life.is_cancelled() {
                    return Err(AppError::new("STALE_OPERATION", "cancelled"));
                }
                state.calls.lock().push((b, ch, op, payload.clone()));
                match (ch, op) {
                    (0, 6) => {
                        assert_eq!(&payload[..4], &[2, 0, 0, 0]);
                        let limit = u64::from_le_bytes(payload[4..].try_into().unwrap());
                        let sent = state.sent.load(Ordering::Acquire);
                        assert!(limit >= sent && limit - sent <= 32768);
                        Ok([sent.to_le_bytes(), limit.to_le_bytes()].concat())
                    }
                    (2, 0x201) => {
                        let n = state.writes.fetch_add(1, Ordering::AcqRel) + 1;
                        if n == state.fail_at.load(Ordering::Acquire) {
                            Err(AppError::uncertain("ACK_TIMEOUT", "timeout"))
                        } else {
                            Ok((payload.len() as u16).to_le_bytes().to_vec())
                        }
                    }
                    (2, 0x202) => Ok([
                        vec![1, 0, 0, 8],
                        9600u32.to_le_bytes().to_vec(),
                        5u32.to_le_bytes().to_vec(),
                        6u32.to_le_bytes().to_vec(),
                    ]
                    .concat()),
                    (2, 0x203) => Ok([7u32.to_le_bytes(), 8u32.to_le_bytes()].concat()),
                    _ => panic!("unexpected opcode"),
                }
            })
        }
    }
    struct FakeBackend {
        state: Arc<Fake>,
        events: broadcast::Sender<DeviceEvent>,
    }
    pub(crate) fn install(app: &mut Arc<crate::service::AppService>) {
        let state = Arc::new(Fake {
            calls: Default::default(),
            sent: AtomicU64::new(0),
            writes: AtomicUsize::new(0),
            fail_at: AtomicUsize::new(usize::MAX),
            live: AtomicBool::new(true),
        });
        let (events, _) = broadcast::channel(256);
        let manager = TtyManager::new(app.active.clone(), Arc::new(FakeBackend { state, events }));
        Arc::get_mut(app).unwrap().tty = manager;
    }
    impl Backend for FakeBackend {
        fn connect(&self, _: &Binding) -> Result<Connection, AppError> {
            Ok(Connection {
                session: 17,
                wire: Arc::new(FakeWire(self.state.clone())),
                events: self.events.subscribe(),
            })
        }
    }
    fn setup() -> (
        Arc<ActiveCoordinator>,
        Arc<TtyManager>,
        Arc<Fake>,
        Principal,
        Binding,
        broadcast::Sender<DeviceEvent>,
    ) {
        let active = Arc::new(ActiveCoordinator::default());
        active.begin().unwrap();
        active.finish(Some(Uid::from_bytes(&[1; 12]).unwrap()));
        let state = Arc::new(Fake {
            calls: Default::default(),
            sent: AtomicU64::new(0),
            writes: AtomicUsize::new(0),
            fail_at: AtomicUsize::new(usize::MAX),
            live: AtomicBool::new(true),
        });
        let (events, _) = broadcast::channel(256);
        let manager = TtyManager::new(
            active.clone(),
            Arc::new(FakeBackend {
                state: state.clone(),
                events: events.clone(),
            }),
        );
        let p = Principal {
            key_id: "test".into(),
            client_id: "rest".into(),
            cancelled: CancellationToken::new(),
            ended: CancellationToken::new(),
        };
        let b = active.bind().unwrap();
        (active, manager, state, p, b, events)
    }
    fn event(b: &Binding, sequence: u32, bytes: Vec<u8>) -> DeviceEvent {
        DeviceEvent {
            uid: b.uid.clone(),
            session: 17,
            frame: Frame {
                kind: 4,
                channel: 2,
                opcode: 0x280,
                session: 17,
                sequence,
                payload: bytes,
            },
        }
    }
    #[tokio::test]
    async fn receive_ring_credit_stop_restart_and_independent_cursors() {
        let (active, m, state, p, b, _events) = setup();
        assert!(state.calls.lock().is_empty());
        let opened = m.open(b.clone(), p.clone()).await.unwrap();
        let c = m.contexts.lock()[&b.uid].clone();
        for n in 0..384u32 {
            state.sent.fetch_add(256, Ordering::AcqRel);
            c.receive(event(&b, n + 1, vec![(n % 256) as u8; 256]));
            if (n + 1) % 64 == 0 {
                c.credit(&active, false).await.unwrap();
            }
        }
        let s = m.cached(&b).unwrap();
        assert_eq!(s.bytes_retained, 65536);
        assert_eq!(s.bytes_overwritten, 32768);
        let a = m.read(&b, Some(&opened.stream_id), Some(0), 4096).unwrap();
        let b_read = m.read(&b, Some(&opened.stream_id), Some(0), 4096).unwrap();
        assert_eq!(a.skipped, 32768);
        assert_eq!(a.bytes, b_read.bytes);
        assert_eq!(a.cursor, 36864);
        assert_eq!(a.bytes[0], 128);
        m.close(&b).unwrap();
        let grants = state.calls.lock().iter().filter(|c| c.2 == 6).count();
        state.sent.fetch_add(1, Ordering::AcqRel);
        c.receive(event(&b, 385, vec![99]));
        tokio::time::sleep(Duration::from_millis(70)).await;
        assert_eq!(
            state.calls.lock().iter().filter(|c| c.2 == 6).count(),
            grants
        );
        assert_eq!(
            m.read(&b, Some(&opened.stream_id), Some(a.cursor), 1)
                .unwrap()
                .bytes,
            vec![144]
        );
        let reopened = m.open(b.clone(), p).await.unwrap();
        assert_ne!(reopened.stream_id, opened.stream_id);
        assert_eq!(
            m.read(&b, Some(&opened.stream_id), Some(0), 1)
                .err()
                .unwrap()
                .code,
            "STALE_STREAM"
        );
        assert!(m.read(&b, None, None, 1).unwrap().bytes.is_empty());
        m.shutdown().await;
    }
    #[tokio::test]
    async fn sequence_loss_and_revocation_stop_credit_without_retarget() {
        let (active, m, state, p, b, events) = setup();
        m.open(b.clone(), p.clone()).await.unwrap();
        state.sent.store(1, Ordering::Release);
        assert!(events.send(event(&b, 2, vec![42])).is_ok());
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(
            m.open(b.clone(), p.clone()).await.err().unwrap().code,
            "TTY_STREAM_GAP"
        );
        assert!(!m.cached(&b).unwrap().receiving);
        active.begin().unwrap();
        m.close_all();
        active.finish(Some(Uid::from_bytes(&[2; 12]).unwrap()));
        assert_eq!(
            m.read(&b, None, None, 1).err().unwrap().code,
            "STALE_OPERATION"
        );
        m.shutdown().await;
        let (_, m, state, p, b, _events) = setup();
        m.open(b.clone(), p.clone()).await.unwrap();
        p.cancelled.cancel();
        tokio::time::sleep(Duration::from_millis(70)).await;
        assert!(!m.cached(&b).unwrap().receiving);
        assert_eq!(state.calls.lock().iter().filter(|c| c.2 == 6).count(), 1);
        m.shutdown().await;
    }
    #[tokio::test]
    async fn exact_chunks_partial_acceptance_and_target_line_coding() {
        let (_, m, state, p, b, _events) = setup();
        let status = m.status(b.clone(), p.clone()).await.unwrap();
        assert_eq!(status.line.baud, 9600);
        assert_eq!(status.line.tx_discarded, 7);
        assert!(!status.receiving);
        let bytes: Vec<_> = (0..65536).map(|n| (n % 251) as u8).collect();
        let sent = m.write(b.clone(), p.clone(), bytes.clone()).await.unwrap();
        assert!(sent.complete);
        assert_eq!(sent.accepted, 65536);
        let wire: Vec<_> = state
            .calls
            .lock()
            .iter()
            .filter(|c| c.2 == 0x201)
            .flat_map(|c| {
                assert!(c.3.len() <= 256);
                c.3.clone()
            })
            .collect();
        assert_eq!(wire, bytes);
        state.fail_at.store(258, Ordering::Release);
        let partial = m.write(b.clone(), p, vec![11; 600]).await.unwrap();
        assert!(!partial.complete);
        assert_eq!(partial.accepted, 256);
        assert!(partial.error.unwrap().uncertain);
        assert_eq!(state.writes.load(Ordering::Acquire), 258);
        m.shutdown().await;
    }
    #[tokio::test]
    async fn accepted_write_queue_is_bounded_and_messages_do_not_interleave() {
        use futures_util::FutureExt;
        let (_, m, state, p, b, _events) = setup();
        for n in 0..4 {
            assert!(m
                .write(b.clone(), p.clone(), vec![n; 512])
                .now_or_never()
                .is_none());
        }
        assert_eq!(
            m.write(b, p, vec![99]).await.err().unwrap().code,
            "QUEUE_FULL"
        );
        tokio::time::timeout(Duration::from_secs(1), async {
            while state.writes.load(Ordering::Acquire) < 8 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let chunks: Vec<_> = state
            .calls
            .lock()
            .iter()
            .filter(|c| c.2 == 0x201)
            .map(|c| c.3[0])
            .collect();
        assert_eq!(chunks, vec![0, 0, 1, 1, 2, 2, 3, 3]);
        m.shutdown().await;
    }
    #[tokio::test]
    async fn tty_remote_schema_rejects_uid_and_preserves_no_active() {
        let (_dir, mut app, _input) = crate::input_api::tests::setup();
        let state = Arc::new(Fake {
            calls: Default::default(),
            sent: AtomicU64::new(0),
            writes: AtomicUsize::new(0),
            fail_at: AtomicUsize::new(usize::MAX),
            live: AtomicBool::new(true),
        });
        let (events, _) = broadcast::channel(256);
        let manager = TtyManager::new(
            app.active.clone(),
            Arc::new(FakeBackend {
                state: state.clone(),
                events,
            }),
        );
        Arc::get_mut(&mut app).unwrap().tty = manager;
        let p = Principal {
            key_id: "A".into(),
            client_id: "rest".into(),
            cancelled: CancellationToken::new(),
            ended: CancellationToken::new(),
        };
        let opened = app
            .execute_remote(&p, "tty_open", serde_json::json!({}))
            .await
            .unwrap();
        assert_eq!(opened["receiving"], true);
        let bad = app
            .execute_remote(
                &p,
                "tty_write",
                serde_json::json!({"bytes":[1],"uid":"other"}),
            )
            .await
            .err()
            .unwrap();
        assert_eq!(bad.code, "INVALID_ARGUMENTS");
        let sent = app
            .execute_remote(&p, "tty_write", serde_json::json!({"bytes":[0,255,13,10]}))
            .await
            .unwrap();
        assert_eq!(sent["accepted"], 4);
        app.select(None).await.unwrap();
        assert_eq!(
            app.execute_remote(&p, "tty_read", serde_json::json!({}))
                .await
                .err()
                .unwrap()
                .code,
            "NO_ACTIVE_CHANNEL"
        );
        assert!(
            !app.tty
                .cached(&Binding {
                    uid: Uid::from_bytes(&[1; 12]).unwrap(),
                    generation: 1
                })
                .unwrap()
                .receiving
        );
        app.tty.shutdown().await;
    }
}
