//! Bounded IPv4 user-space gateway, translated into smoltcp TCP/UDP sockets.
use super::{
    flow_budget::{Budget, Permit},
    host_io::{Host, Message, Pipe, Retired},
    ipv4::{self, Key},
    Config,
};
use crate::model::AppError;
use smoltcp::{
    iface::{SocketHandle, SocketSet},
    socket::{tcp, udp},
    wire::{IpAddress, IpEndpoint},
};
use std::{
    net::{Ipv4Addr, SocketAddr},
    sync::Arc,
};
use tokio_util::sync::CancellationToken;
#[cfg(test)]
mod tests;
pub const TCP_SLOTS: usize = 8;
pub const UDP_SLOTS: usize = 24;
const TCP_BASE: u16 = 20000;
const UDP_BASE: u16 = 20032;
struct Flow {
    _permit: Permit,
    key: Key,
    handle: SocketHandle,
    last: u64,
    created: u64,
    pipe: Option<Pipe>,
    pending: Option<(Vec<u8>, usize)>,
    sent_fin: bool,
    closing: Option<u64>,
}
pub struct Gateway {
    budget: Budget,
    retired: Retired,
    config: Config,
    server: Ipv4Addr,
    host: Arc<Host>,
    life: CancellationToken,
    tcp: Vec<Option<Flow>>,
    udp: Vec<Option<Flow>>,
    pub dropped: u64,
}
impl Drop for Gateway {
    fn drop(&mut self) {
        self.life.cancel();
    }
}
impl Gateway {
    pub fn new(config: Config, host: Host) -> Result<Self, AppError> {
        Self::scoped(config, host, CancellationToken::new())
    }
    pub fn scoped(config: Config, host: Host, life: CancellationToken) -> Result<Self, AppError> {
        Self::budgeted(config, host, life, Budget::default())
    }
    pub fn budgeted(
        config: Config,
        host: Host,
        life: CancellationToken,
        budget: Budget,
    ) -> Result<Self, AppError> {
        Ok(Self {
            budget,
            retired: Retired::default(),
            server: config.validate()?,
            config,
            host: Arc::new(host),
            life: life.child_token(),
            tcp: (0..TCP_SLOTS).map(|_| None).collect(),
            udp: (0..UDP_SLOTS).map(|_| None).collect(),
            dropped: 0,
        })
    }
    pub fn count(&self) -> usize {
        self.tcp.iter().chain(self.udp.iter()).flatten().count()
    }
    /// Parse and validate before opening any host socket or reserving a flow.
    pub fn ingress(
        &mut self,
        frame: Vec<u8>,
        sockets: &mut SocketSet<'static>,
        now: u64,
    ) -> Option<Vec<u8>> {
        if frame.get(12..14) != Some(&[8, 0][..]) {
            return Some(frame);
        }
        let Some(p) = ipv4::packet(&frame) else {
            self.dropped += 1;
            return None;
        };
        let k = p.key;
        if k.remote == self.server
            && ((TCP_BASE..TCP_BASE + TCP_SLOTS as u16).contains(&k.remote_port)
                || (UDP_BASE..UDP_BASE + UDP_SLOTS as u16).contains(&k.remote_port))
        {
            self.dropped += 1;
            return None;
        }
        let dns = k.remote == self.server && k.remote_port == 53;
        if k.remote.octets()[..3] == self.server.octets()[..3] && !dns {
            return Some(frame);
        }
        let oct = k.remote.octets();
        if self.config.target(k.client).is_err()
            || k.client_port == 0
            || k.remote_port == 0
            || p.frame[22] <= 1
            || !p.valid()
            || !dns && (oct[0] == 0 || oct[0] == 127 || oct[0] >= 224 || oct[..2] == [169, 254])
        {
            self.dropped += 1;
            return None;
        }
        let slots = if k.protocol == 6 {
            &mut self.tcp
        } else {
            &mut self.udp
        };
        let index = if let Some(index) = slots.iter().position(|f| {
            f.as_ref()
                .is_some_and(|f| f.key == k && f.closing.is_none())
        }) {
            index
        } else {
            if k.protocol == 6 && !p.syn() {
                self.dropped += 1;
                return None;
            }
            let Some(index) = slots.iter().position(Option::is_none) else {
                self.dropped += 1;
                return None;
            };
            let Some(permit) = self.budget.reserve() else {
                self.dropped += 1;
                return None;
            };
            let port = if k.protocol == 6 { TCP_BASE } else { UDP_BASE } + index as u16;
            let (handle, pipe) = if k.protocol == 6 {
                let mut s = tcp::Socket::new(
                    tcp::SocketBuffer::new(vec![0; 16384]),
                    tcp::SocketBuffer::new(vec![0; 16384]),
                );
                s.set_timeout(Some(smoltcp::time::Duration::from_secs(120)));
                if s.listen((self.server, port)).is_err() {
                    self.dropped += 1;
                    return None;
                }
                (sockets.add(s), None)
            } else {
                let mut s = udp::Socket::new(
                    udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 8], vec![0; 1472 * 8]),
                    udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 8], vec![0; 1472 * 8]),
                );
                if s.bind((self.server, port)).is_err() {
                    self.dropped += 1;
                    return None;
                }
                let address = if dns {
                    self.host.dns
                } else {
                    SocketAddr::from((k.remote, k.remote_port))
                };
                (
                    sockets.add(s),
                    Some(
                        self.host
                            .pipe_budgeted(
                                k.protocol,
                                address,
                                self.life.child_token(),
                                Some(permit.clone()),
                            )
                            .retire_to(self.retired.clone()),
                    ),
                )
            };
            slots[index] = Some(Flow {
                _permit: permit,
                key: k,
                handle,
                last: now,
                created: now,
                pipe,
                pending: None,
                sent_fin: false,
                closing: None,
            });
            index
        };
        let f = slots[index].as_mut().unwrap();
        f.last = now;
        Some(p.rewrite(
            k.client,
            k.client_port,
            self.server,
            if k.protocol == 6 { TCP_BASE } else { UDP_BASE } + index as u16,
        ))
    }
    pub fn egress(&mut self, frame: Vec<u8>) -> Option<Vec<u8>> {
        let Some(p) = ipv4::packet(&frame) else {
            return Some(frame);
        };
        if p.key.client != self.server {
            return Some(frame);
        }
        let base = if p.key.protocol == 6 {
            TCP_BASE
        } else {
            UDP_BASE
        };
        let slots = if p.key.protocol == 6 {
            &self.tcp
        } else {
            &self.udp
        };
        let Some(index) = p
            .key
            .client_port
            .checked_sub(base)
            .map(usize::from)
            .filter(|i| *i < slots.len())
        else {
            return Some(frame);
        };
        let Some(f) = &slots[index] else {
            self.dropped += 1;
            return None;
        };
        if f.key.client != p.key.remote || f.key.client_port != p.key.remote_port {
            self.dropped += 1;
            return None;
        }
        Some(p.rewrite(
            f.key.remote,
            f.key.remote_port,
            f.key.client,
            f.key.client_port,
        ))
    }
    pub fn poll(&mut self, sockets: &mut SocketSet<'static>, now: u64) {
        self.poll_queued(sockets, now, true);
    }
    pub fn poll_queued(&mut self, sockets: &mut SocketSet<'static>, now: u64, allow_reuse: bool) {
        for (protocol, slots) in [(6, &mut self.tcp), (17, &mut self.udp)] {
            for slot in slots {
                let Some(f) = slot else {
                    continue;
                };
                if let Some(pipe) = &f.pipe {
                    self.dropped += pipe.dropped.swap(0, std::sync::atomic::Ordering::AcqRel);
                }
                if let Some(at) = f.closing {
                    // Keep the reverse mapping long enough to serialize a reset before reuse.
                    if allow_reuse && now.saturating_sub(at) >= 100 {
                        sockets.remove(f.handle);
                        *slot = None;
                    }
                    continue;
                }
                let idle = if protocol == 6 { 120_000 } else { 30_000 };
                let expire = now.saturating_sub(f.last) >= idle
                    || protocol == 6 && f.pipe.is_none() && now.saturating_sub(f.created) >= 10_000;
                let result = if expire {
                    Err(())
                } else if protocol == 6 {
                    pump_tcp(
                        f,
                        sockets,
                        &self.host,
                        &self.life,
                        now,
                        self.server,
                        &self.retired,
                    )
                } else {
                    pump_udp(f, sockets, now)
                };
                if result.is_err() {
                    self.dropped += 1;
                    f.pipe = None;
                    f.pending = None;
                    f.closing = Some(now);
                    if protocol == 6 {
                        sockets.get_mut::<tcp::Socket>(f.handle).abort();
                    } else {
                        sockets.get_mut::<udp::Socket>(f.handle).close();
                    }
                }
            }
        }
    }
    pub async fn shutdown(mut self) {
        self.life.cancel();
        let mut tasks = vec![];
        for flow in self.tcp.iter_mut().chain(self.udp.iter_mut()).flatten() {
            if let Some(pipe) = flow.pipe.take() {
                if let Some(task) = pipe.shutdown() {
                    tasks.push(task);
                }
            }
        }
        for task in tasks {
            let _ = task.await;
        }
        self.retired.finish().await;
    }
}
fn pump_tcp(
    f: &mut Flow,
    sockets: &mut SocketSet<'static>,
    host: &Host,
    life: &CancellationToken,
    now: u64,
    server: Ipv4Addr,
    retired: &Retired,
) -> Result<(), ()> {
    let s = sockets.get_mut::<tcp::Socket>(f.handle);
    if f.pipe.is_none() {
        if s.state() == tcp::State::Closed {
            return Err(());
        }
        if ![tcp::State::Established, tcp::State::CloseWait].contains(&s.state()) {
            return Ok(());
        }
        let address = if f.key.remote == server && f.key.remote_port == 53 {
            host.dns
        } else {
            SocketAddr::from((f.key.remote, f.key.remote_port))
        };
        f.pipe = Some(
            host.pipe_budgeted(6, address, life.child_token(), Some(f._permit.clone()))
                .retire_to(retired.clone()),
        );
    }
    let pipe = f.pipe.as_mut().unwrap();
    if pipe.is_finished() {
        // try_join's successful completion means both half-closes are queued, not delivered yet.
        if pipe.failed() {
            return Err(());
        }
        // The cached result prevents polling a completed JoinHandle a second time.
        if s.state() == tcp::State::Closed {
            if pipe.succeeded() {
                f.pipe = None;
                f.closing = Some(now);
                return Ok(());
            }
            return Err(());
        }
    }
    if s.can_recv() {
        if let Ok(permit) = pipe.tx.try_reserve() {
            let mut b = vec![0; 4096];
            let n = s.recv_slice(&mut b).map_err(|_| ())?;
            b.truncate(n);
            permit.send(Message::Data(b));
            f.last = now;
        }
    } else if !s.may_recv() && !f.sent_fin {
        if let Ok(permit) = pipe.tx.try_reserve() {
            permit.send(Message::Fin);
            f.sent_fin = true;
            f.last = now;
        }
    }
    if f.pending.is_none() {
        match pipe.rx.try_recv() {
            Ok(Message::Data(b)) => {
                f.pending = Some((b, 0));
                f.last = now;
            }
            Ok(Message::Fin) => {
                s.close();
                f.last = now;
            }
            Err(_) => {}
        }
    }
    if let Some((b, offset)) = &mut f.pending {
        if s.can_send() {
            let n = s.send_slice(&b[*offset..]).map_err(|_| ())?;
            *offset += n;
            f.last = now;
        }
        if *offset == b.len() {
            f.pending = None;
        }
    }
    Ok(())
}
fn pump_udp(f: &mut Flow, sockets: &mut SocketSet<'static>, now: u64) -> Result<(), ()> {
    let pipe = f.pipe.as_mut().ok_or(())?;
    if pipe.is_finished() {
        return Err(());
    }
    let s = sockets.get_mut::<udp::Socket>(f.handle);
    if s.can_recv() {
        if let Ok(permit) = pipe.tx.try_reserve() {
            let mut b = vec![0; 1472];
            let (n, meta) = s.recv_slice(&mut b).map_err(|_| ())?;
            if meta.endpoint == IpEndpoint::new(IpAddress::Ipv4(f.key.client), f.key.client_port) {
                b.truncate(n);
                permit.send(Message::Data(b));
                f.last = now;
            }
        }
    }
    if f.pending.is_none() {
        if let Ok(Message::Data(b)) = pipe.rx.try_recv() {
            f.pending = Some((b, 0));
        }
    }
    if let Some((b, _)) = &f.pending {
        if s.can_send() {
            s.send_slice(
                b,
                IpEndpoint::new(IpAddress::Ipv4(f.key.client), f.key.client_port),
            )
            .map_err(|_| ())?;
            f.pending = None;
            f.last = now;
        }
    }
    Ok(())
}
