//! New loopback listeners to one selected NCM stack, never existing host services.
use super::{
    flow_budget::{Budget, Permit},
    host_io::{Message, Pipe, Retired},
    ipv4,
    scanner::{Kind, Scope},
    Config,
};
use crate::{input_sessions::Principal, model::AppError};
use serde::{Deserialize, Serialize};
use smoltcp::{
    iface::{Interface, SocketHandle, SocketSet},
    socket::{tcp, udp},
    wire::{IpAddress, IpEndpoint},
};
use std::{
    collections::{HashMap, HashSet},
    net::{Ipv4Addr, SocketAddr},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};
use tokio::{
    net::{TcpListener, UdpSocket},
    sync::mpsc,
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;
#[cfg(test)]
mod tests;
const BASE: u16 = 47000;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    Tcp,
    Udp,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub id: String,
    pub protocol: Protocol,
    pub listen_port: u16,
    pub target_ip: String,
    pub target_port: u16,
}
pub fn validate(rules: &[Rule], config: &Config) -> Result<(), AppError> {
    if rules.is_empty() || rules.len() > 32 {
        return Err(invalid());
    }
    let mut ids = HashSet::new();
    let mut binds = HashSet::new();
    for rule in rules {
        let ip = rule.target_ip.parse::<Ipv4Addr>().map_err(|_| invalid())?;
        if rule.id.is_empty()
            || rule.id.len() > 64
            || !rule
                .id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
            || !ids.insert(&rule.id)
            || !binds.insert((rule.protocol, rule.listen_port))
            || rule.listen_port == 0
            || rule.target_port == 0
            || rule.target_ip != ip.to_string()
        {
            return Err(invalid());
        }
        config.target(ip)?;
    }
    Ok(())
}
fn invalid() -> AppError {
    AppError::new(
        "INVALID_ARGUMENTS",
        "포워딩 규칙은1..32개, 고유 ID/로컬 TCP·UDP 포트와 NCM /24 타깃 주소·포트가 필요합니다.",
    )
}
#[derive(Clone, Debug, Default, Serialize, schemars::JsonSchema)]
pub struct Status {
    pub running: bool,
    pub cleanup_required: bool,
    pub rules: Vec<Rule>,
    pub connections: usize,
    pub dropped: u64,
    pub error: Option<AppError>,
}
enum Listener {
    Tcp(TcpListener),
    Udp(UdpSocket),
}
pub struct Bound {
    rules: Vec<Rule>,
    listeners: Vec<Listener>,
}
impl Bound {
    pub async fn bind(rules: Vec<Rule>, config: &Config) -> Result<Self, AppError> {
        validate(&rules, config)?;
        let mut listeners = vec![];
        for rule in &rules {
            let address = SocketAddr::from((Ipv4Addr::LOCALHOST, rule.listen_port));
            let listener = match rule.protocol {
                Protocol::Tcp => Listener::Tcp(
                    TcpListener::bind(address)
                        .await
                        .map_err(|_| bind_error(rule))?,
                ),
                Protocol::Udp => Listener::Udp(
                    UdpSocket::bind(address)
                        .await
                        .map_err(|_| bind_error(rule))?,
                ),
            };
            listeners.push(listener);
        }
        Ok(Self { rules, listeners })
    }
}
fn bind_error(rule: &Rule) -> AppError {
    AppError::new(
        "FORWARD_BIND_FAILED",
        format!(
            "로컬 {:?} {} 포트를 열지 못했습니다. 기존 수신 포트와 충돌하는지 확인하세요.",
            rule.protocol, rule.listen_port
        ),
    )
}
struct Incoming {
    rule: Rule,
    pipe: Pipe,
    permit: Permit,
    life: CancellationToken,
}
struct Runtime {
    life: CancellationToken,
    drivers: Vec<JoinHandle<()>>,
    events: mpsc::Receiver<Incoming>,
    drops: Arc<AtomicU64>,
    retired: Retired,
}
impl Drop for Runtime {
    fn drop(&mut self) {
        self.life.cancel();
        for driver in &self.drivers {
            driver.abort();
        }
    }
}
pub struct Cleanup {
    runtime: Option<Runtime>,
    pipes: Vec<Pipe>,
}
impl Cleanup {
    pub async fn finish(mut self) {
        let mut tasks = vec![];
        let retired = self.runtime.as_ref().map(|runtime| runtime.retired.clone());
        if let Some(mut runtime) = self.runtime.take() {
            runtime.life.cancel();
            for driver in &runtime.drivers {
                driver.abort();
            }
            for driver in runtime.drivers.drain(..) {
                let _ = driver.await;
            }
            while let Ok(incoming) = runtime.events.try_recv() {
                self.pipes.push(incoming.pipe);
            }
        }
        for pipe in self.pipes.drain(..) {
            if let Some(task) = pipe.shutdown() {
                tasks.push(task);
            }
        }
        for task in tasks {
            let _ = task.await;
        }
        if let Some(retired) = retired {
            retired.finish().await;
        }
    }
}
struct Flow {
    rule: Rule,
    port: u16,
    handle: SocketHandle,
    pipe: Pipe,
    permit: Permit,
    scope: Scope,
    last: u64,
    created: u64,
    pending: Option<(Vec<u8>, usize)>,
    fin: bool,
    established: bool,
}
pub struct Forwarder {
    config: Config,
    server: Ipv4Addr,
    budget: Budget,
    runtime: Option<Runtime>,
    flows: Vec<Flow>,
    retired: HashMap<u16, u64>,
    next: u16,
    scope: Option<Scope>,
    status: Status,
}
impl Drop for Forwarder {
    fn drop(&mut self) {
        if let Some(scope) = &self.scope {
            scope.life.cancel();
        }
    }
}
impl Forwarder {
    pub fn new(config: Config, budget: Budget) -> Result<Self, AppError> {
        Ok(Self {
            server: config.validate()?,
            config,
            budget,
            runtime: None,
            flows: vec![],
            retired: HashMap::new(),
            next: BASE,
            scope: None,
            status: Default::default(),
        })
    }
    pub fn status(&self) -> Status {
        let mut status = self.status.clone();
        status.connections = self.flows.len();
        status.cleanup_required = self.runtime.is_some();
        if let Some(runtime) = &self.runtime {
            status.dropped += runtime.drops.load(Ordering::Acquire);
        }
        status
    }
    pub fn start(
        &mut self,
        bound: Bound,
        owner: Principal,
        life: CancellationToken,
    ) -> Result<CancellationToken, AppError> {
        if self.runtime.is_some() {
            return Err(AppError::new(
                "FORWARD_BUSY",
                "기존 포워딩을 먼저 중지하세요.",
            ));
        }
        validate(&bound.rules, &self.config)?;
        if life.is_cancelled() || owner.cancelled.is_cancelled() || owner.ended.is_cancelled() {
            return Err(AppError::new(
                "OPERATION_CANCELLED",
                "포워딩 요청 연결이 종료됐습니다.",
            ));
        }
        let token = life.child_token();
        let (events, receiver) = mpsc::channel(32);
        let drops = Arc::new(AtomicU64::new(0));
        let mut drivers = vec![];
        let retired = Retired::default();
        for (rule, listener) in bound.rules.iter().cloned().zip(bound.listeners) {
            let events = events.clone();
            let budget = self.budget.clone();
            let cancel = token.child_token();
            let dropped = drops.clone();
            let retired = retired.clone();
            drivers.push(tokio::spawn(async move {match listener {
                Listener::Tcp(listener)=>loop {
                    let accepted=tokio::select!{biased;_=cancel.cancelled()=>break,result=listener.accept()=>result};
                    let Ok((socket,remote))=accepted else {break;};if !remote.ip().is_loopback() {continue;}
                    let Some(permit)=budget.reserve() else {dropped.fetch_add(1,Ordering::Relaxed);continue;};
                    let life=cancel.child_token();let pipe=Pipe::accepted(socket,life.child_token(),permit.clone()).retire_to(retired.clone());
                    if events.try_send(Incoming{rule:rule.clone(),pipe,permit,life}).is_err() {dropped.fetch_add(1,Ordering::Relaxed);}
                },
                Listener::Udp(socket)=>{
                    let socket=Arc::new(socket);let mut clients:HashMap<SocketAddr,mpsc::Sender<Message>>=HashMap::new();
                    loop {
                        let mut b=vec![0;1473];let received=tokio::select!{biased;_=cancel.cancelled()=>break,result=socket.recv_from(&mut b)=>result};
                        let Ok((n,remote))=received else {break;};if !remote.ip().is_loopback() || n>1472 {dropped.fetch_add(1,Ordering::Relaxed);continue;}b.truncate(n);
                        clients.retain(|_,sender|!sender.is_closed());
                        if let std::collections::hash_map::Entry::Vacant(entry)=clients.entry(remote) {
                            let Some(permit)=budget.reserve() else {dropped.fetch_add(1,Ordering::Relaxed);continue;};
                            let life=cancel.child_token();let (pipe,sender)=Pipe::udp_client(socket.clone(),remote,life.child_token(),permit.clone());let pipe=pipe.retire_to(retired.clone());
                            if events.try_send(Incoming{rule:rule.clone(),pipe,permit,life}).is_err() {dropped.fetch_add(1,Ordering::Relaxed);continue;}
                            entry.insert(sender);
                        }
                        if clients[&remote].try_send(Message::Data(b)).is_err() {dropped.fetch_add(1,Ordering::Relaxed);}
                    }
                }
            }}));
        }
        self.status = Status {
            running: true,
            rules: bound.rules,
            ..Default::default()
        };
        self.scope = Some(Scope {
            life: token.clone(),
            owner,
            kind: Kind::Forward,
        });
        self.runtime = Some(Runtime {
            life: token.clone(),
            drivers,
            events: receiver,
            drops,
            retired,
        });
        Ok(token)
    }
    pub fn stop(&mut self, sockets: &mut SocketSet<'static>) -> Cleanup {
        if let Some(scope) = self.scope.take() {
            scope.life.cancel();
        }
        let mut pipes = vec![];
        for flow in self.flows.drain(..) {
            flow.scope.life.cancel();
            sockets.remove(flow.handle);
            pipes.push(flow.pipe);
        }
        if let Some(runtime) = &self.runtime {
            runtime.life.cancel();
        }
        self.status = self.status();
        self.status.running = false;
        self.status.connections = 0;
        Cleanup {
            runtime: self.runtime.take(),
            pipes,
        }
    }
    fn port(&mut self, now: u64) -> Option<u16> {
        for _ in BASE..=u16::MAX {
            let port = self.next;
            self.next = self.next.checked_add(1).unwrap_or(BASE);
            if self.flows.iter().any(|flow| flow.port == port) {
                continue;
            }
            if self.retired.get(&port).is_some_and(|until| *until > now) {
                continue;
            }
            self.retired.remove(&port);
            return Some(port);
        }
        None
    }
    pub fn scope_for(&self, frame: &[u8]) -> Option<Scope> {
        if let Some(p) = ipv4::packet(frame) {
            if p.key.client == self.server && p.key.client_port >= BASE {
                return self
                    .flows
                    .iter()
                    .find(|flow| {
                        flow.port == p.key.client_port
                            && flow.rule.target_ip == p.key.remote.to_string()
                            && flow.rule.target_port == p.key.remote_port
                    })
                    .map(|flow| flow.scope.clone());
            }
        } else if frame.len() >= 42 && frame[12..22] == [8, 6, 0, 1, 8, 0, 6, 4, 0, 1] {
            let ip = Ipv4Addr::new(frame[38], frame[39], frame[40], frame[41]);
            if self
                .flows
                .iter()
                .any(|flow| flow.rule.target_ip == ip.to_string())
            {
                return self.scope.clone();
            }
        }
        None
    }
    pub fn reserved_output(&self, frame: &[u8]) -> bool {
        ipv4::packet(frame)
            .is_some_and(|p| p.key.client == self.server && p.key.client_port >= BASE)
    }
    pub fn reject_input(&self, frame: &[u8]) -> bool {
        let Some(p) = ipv4::packet(frame) else {
            return false;
        };
        if p.key.remote != self.server || p.key.remote_port < BASE {
            return false;
        }
        !self.flows.iter().any(|flow| {
            flow.port == p.key.remote_port
                && flow.rule.target_ip == p.key.client.to_string()
                && flow.rule.target_port == p.key.client_port
                && match flow.rule.protocol {
                    Protocol::Tcp => p.key.protocol == 6,
                    Protocol::Udp => p.key.protocol == 17,
                }
        })
    }
    pub fn poll(&mut self, sockets: &mut SocketSet<'static>, interface: &mut Interface, now: u64) {
        if let Some(runtime) = &self.runtime {
            if self.status.running
                && !runtime.life.is_cancelled()
                && runtime.drivers.iter().any(JoinHandle::is_finished)
            {
                runtime.life.cancel();
                self.status.running = false;
                self.status.error = Some(AppError::new(
                    "FORWARD_LISTENER_FAILED",
                    "포워딩 수신 포트가 종료됐습니다. 중지 후 다시 시작하세요.",
                ));
            }
        }
        for _ in 0..32 {
            let event = self
                .runtime
                .as_mut()
                .and_then(|runtime| runtime.events.try_recv().ok());
            let Some(incoming) = event else {
                break;
            };
            if incoming.life.is_cancelled() || self.scope.as_ref().is_none_or(Scope::cancelled) {
                continue;
            }
            let Some(port) = self.port(now) else {
                self.status.dropped += 1;
                continue;
            };
            let remote: Ipv4Addr = incoming
                .rule
                .target_ip
                .parse()
                .expect("validated rule IPv4");
            let handle = match incoming.rule.protocol {
                Protocol::Tcp => {
                    let mut socket = tcp::Socket::new(
                        tcp::SocketBuffer::new(vec![0; 16384]),
                        tcp::SocketBuffer::new(vec![0; 16384]),
                    );
                    socket.set_timeout(Some(smoltcp::time::Duration::from_secs(120)));
                    if socket
                        .connect(
                            interface.context(),
                            (remote, incoming.rule.target_port),
                            (self.server, port),
                        )
                        .is_err()
                    {
                        self.status.dropped += 1;
                        continue;
                    }
                    sockets.add(socket)
                }
                Protocol::Udp => {
                    let mut socket = udp::Socket::new(
                        udp::PacketBuffer::new(
                            vec![udp::PacketMetadata::EMPTY; 8],
                            vec![0; 1472 * 8],
                        ),
                        udp::PacketBuffer::new(
                            vec![udp::PacketMetadata::EMPTY; 8],
                            vec![0; 1472 * 8],
                        ),
                    );
                    if socket.bind((self.server, port)).is_err() {
                        self.status.dropped += 1;
                        continue;
                    }
                    sockets.add(socket)
                }
            };
            let scope = Scope {
                life: incoming.life,
                owner: self.scope.as_ref().unwrap().owner.clone(),
                kind: Kind::Forward,
            };
            self.flows.push(Flow {
                rule: incoming.rule,
                port,
                handle,
                pipe: incoming.pipe,
                permit: incoming.permit,
                scope,
                last: now,
                created: now,
                pending: None,
                fin: false,
                established: false,
            });
        }
        let mut at = 0;
        while at < self.flows.len() {
            let flow = &mut self.flows[at];
            let idle = if flow.rule.protocol == Protocol::Tcp {
                120_000
            } else {
                30_000
            };
            let result = if flow.scope.cancelled() || now.saturating_sub(flow.last) >= idle {
                Ok(true)
            } else {
                match flow.rule.protocol {
                    Protocol::Tcp => pump_tcp(flow, sockets, now),
                    Protocol::Udp => pump_udp(flow, sockets, now),
                }
            };
            match result {
                Ok(false) => at += 1,
                other => {
                    if other.is_err() {
                        self.status.dropped += 1;
                        self.status.error = Some(AppError::new(
                            "FORWARD_CONNECTION_FAILED",
                            "포워딩 연결이 종료되거나 타깃의 응답 시간이 초과됐습니다.",
                        ));
                    }
                    let flow = self.flows.swap_remove(at);
                    flow.scope.life.cancel();
                    sockets.remove(flow.handle);
                    self.retired.insert(flow.port, now.saturating_add(60_000));
                    drop(flow.permit);
                }
            }
        }
    }
}
fn pump_tcp(flow: &mut Flow, sockets: &mut SocketSet<'static>, now: u64) -> Result<bool, ()> {
    let socket = sockets.get_mut::<tcp::Socket>(flow.handle);
    if [tcp::State::Established, tcp::State::CloseWait].contains(&socket.state()) {
        flow.established = true;
    }
    if flow.pipe.failed() {
        return Err(());
    }
    if flow.pipe.succeeded() && [tcp::State::TimeWait, tcp::State::Closed].contains(&socket.state())
    {
        return Ok(true);
    }
    if socket.state() == tcp::State::Closed
        || !flow.established && now.saturating_sub(flow.created) >= 15_000
    {
        return Err(());
    }
    if !flow.established {
        return Ok(false);
    }
    if socket.can_recv() {
        if let Ok(permit) = flow.pipe.tx.try_reserve() {
            let mut b = vec![0; 4096];
            let n = socket.recv_slice(&mut b).map_err(|_| ())?;
            b.truncate(n);
            permit.send(Message::Data(b));
            flow.last = now;
        }
    } else if !socket.may_recv() && !flow.fin {
        if let Ok(permit) = flow.pipe.tx.try_reserve() {
            permit.send(Message::Fin);
            flow.fin = true;
            flow.last = now;
        }
    }
    if flow.pending.is_none() {
        match flow.pipe.rx.try_recv() {
            Ok(Message::Data(b)) => {
                flow.pending = Some((b, 0));
                flow.last = now;
            }
            Ok(Message::Fin) => {
                socket.close();
                flow.last = now;
            }
            Err(_) => {}
        }
    }
    if let Some((b, offset)) = &mut flow.pending {
        if socket.can_send() {
            let n = socket.send_slice(&b[*offset..]).map_err(|_| ())?;
            *offset += n;
            flow.last = now;
        }
        if *offset == b.len() {
            flow.pending = None;
        }
    }
    Ok(false)
}
fn pump_udp(flow: &mut Flow, sockets: &mut SocketSet<'static>, now: u64) -> Result<bool, ()> {
    if flow.pipe.is_finished() {
        return Err(());
    }
    let socket = sockets.get_mut::<udp::Socket>(flow.handle);
    let target = IpEndpoint::new(
        IpAddress::Ipv4(flow.rule.target_ip.parse().expect("validated IPv4")),
        flow.rule.target_port,
    );
    if socket.can_recv() {
        if let Ok(permit) = flow.pipe.tx.try_reserve() {
            let mut b = vec![0; 1472];
            let (n, meta) = socket.recv_slice(&mut b).map_err(|_| ())?;
            if meta.endpoint == target {
                b.truncate(n);
                permit.send(Message::Data(b));
                flow.last = now;
            }
        }
    }
    if flow.pending.is_none() {
        if let Ok(Message::Data(b)) = flow.pipe.rx.try_recv() {
            flow.pending = Some((b, 0));
        }
    }
    if let Some((b, _)) = &flow.pending {
        if socket.can_send() {
            socket.send_slice(b, target).map_err(|_| ())?;
            flow.pending = None;
            flow.last = now;
        }
    }
    Ok(false)
}
