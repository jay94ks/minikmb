//! A bounded in-process Ethernet device; never opens a host NIC or TAP device.
use super::{
    dhcp,
    flow_budget::Budget,
    forwarding,
    gateway::Gateway,
    host_io::Host,
    scanner::{self, Output, Scanner},
    Config,
};
use crate::model::AppError;
use smoltcp::{
    iface::{Config as InterfaceConfig, Interface, SocketSet},
    phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken},
    time::Instant,
    wire::{EthernetAddress, IpCidr, Ipv4Cidr},
};
use std::collections::VecDeque;
#[cfg(test)]
mod tests;
pub const QUEUE: usize = 32;
pub struct QueueDevice {
    rx: VecDeque<Vec<u8>>,
    tx: VecDeque<Vec<u8>>,
    tx_limit: usize,
    pub dropped: u64,
}
pub struct Rx(Vec<u8>);
pub struct Tx<'a>(&'a mut VecDeque<Vec<u8>>);
impl RxToken for Rx {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        f(&self.0)
    }
}
impl TxToken for Tx<'_> {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let mut b = vec![0; len];
        let r = f(&mut b);
        self.0.push_back(b);
        r
    }
}
impl Device for QueueDevice {
    type RxToken<'a> = Rx;
    type TxToken<'a> = Tx<'a>;
    fn receive(&mut self, _now: Instant) -> Option<(Rx, Tx<'_>)> {
        if self.tx.len() >= self.tx_limit {
            return None;
        }
        let b = self.rx.pop_front()?;
        Some((Rx(b), Tx(&mut self.tx)))
    }
    fn transmit(&mut self, _now: Instant) -> Option<Tx<'_>> {
        if self.tx.len() >= self.tx_limit {
            None
        } else {
            Some(Tx(&mut self.tx))
        }
    }
    fn capabilities(&self) -> DeviceCapabilities {
        let mut c = DeviceCapabilities::default();
        c.medium = Medium::Ethernet;
        c.max_transmission_unit = 1514;
        c.max_burst_size = Some(QUEUE);
        c
    }
}
pub struct Engine {
    pub(super) device: QueueDevice,
    pub(super) interface: Interface,
    pub(super) sockets: SocketSet<'static>,
    dhcp: dhcp::Server,
    ready: VecDeque<Output>,
    gateway: Option<Gateway>,
    scanner: Scanner,
    budget: Budget,
    pub(super) forward: forwarding::Forwarder,
}
impl Engine {
    pub fn new(config: Config, mac: [u8; 6], host: bool) -> Result<Self, AppError> {
        let ip = config.validate()?;
        if mac[0] & 1 != 0 || mac == [0; 6] {
            return Err(AppError::new(
                "INVALID_ARGUMENTS",
                "가상 네트워크에는 유효한 유니캐스트 MAC이 필요합니다.",
            ));
        }
        let mut device = QueueDevice {
            rx: VecDeque::with_capacity(QUEUE),
            tx: VecDeque::with_capacity(QUEUE),
            tx_limit: QUEUE,
            dropped: 0,
        };
        let mut c = InterfaceConfig::new(EthernetAddress(mac).into());
        let mut seed = [0; 8];
        getrandom::fill(&mut seed)
            .map_err(|_| AppError::new("RANDOM_FAILED", "네트워크 난수를 준비하지 못했습니다."))?;
        c.random_seed = u64::from_le_bytes(seed);
        let mut interface = Interface::new(c, &mut device, Instant::from_millis(0));
        interface.update_ip_addrs(|ips| {
            ips.push(IpCidr::Ipv4(Ipv4Cidr::new(ip, 24)))
                .expect("one IPv4 address");
        });
        let scanner = Scanner::new(config.clone(), mac)?;
        let budget = Budget::default();
        let forward = forwarding::Forwarder::new(config.clone(), budget.clone())?;
        Ok(Self {
            device,
            interface,
            sockets: SocketSet::new(vec![]),
            dhcp: dhcp::Server::new(config, mac, host)?,
            ready: VecDeque::with_capacity(QUEUE),
            gateway: None,
            scanner,
            budget,
            forward,
        })
    }
    pub fn with_host(config: Config, mac: [u8; 6], host: Host) -> Result<Self, AppError> {
        Self::with_host_scoped(
            config,
            mac,
            host,
            tokio_util::sync::CancellationToken::new(),
        )
    }
    pub fn with_host_scoped(
        config: Config,
        mac: [u8; 6],
        host: Host,
        life: tokio_util::sync::CancellationToken,
    ) -> Result<Self, AppError> {
        let mut engine = Self::new(config.clone(), mac, true)?;
        engine.gateway = Some(Gateway::budgeted(
            config,
            host,
            life,
            engine.budget.clone(),
        )?);
        Ok(engine)
    }
    pub fn receive(&mut self, frame: Vec<u8>, now: u64) {
        if frame.len() < 14 || frame.len() > 1514 {
            self.device.dropped += 1;
            return;
        }
        if self.scanner.receive(&frame) {
            return;
        }
        if self.forward.reject_input(&frame) {
            self.device.dropped += 1;
            return;
        }
        match self.dhcp.handle(&frame, now) {
            dhcp::Reply::Consumed(Some(reply)) => {
                if self.ready.len() + self.device.tx.len() < QUEUE {
                    self.ready.push_back(reply.into());
                } else {
                    self.device.dropped += 1;
                }
                return;
            }
            dhcp::Reply::Consumed(None) => return,
            dhcp::Reply::Other => {}
        }
        if frame[12] != 8 || ![0, 6].contains(&frame[13]) {
            return;
        }
        let frame = if let Some(gateway) = &mut self.gateway {
            let Some(frame) = gateway.ingress(frame, &mut self.sockets, now) else {
                return;
            };
            frame
        } else {
            frame
        };
        if self.device.rx.len() >= QUEUE {
            self.device.dropped += 1;
            return;
        }
        self.device.rx.push_back(frame);
        self.poll(now);
    }
    pub fn poll(&mut self, now: u64) {
        self.ready
            .retain(|item| item.scope.as_ref().is_none_or(|scope| !scope.cancelled()));
        let space = QUEUE.saturating_sub(self.ready.len() + self.device.tx.len());
        self.ready.extend(self.scanner.poll(now, space));
        self.forward
            .poll(&mut self.sockets, &mut self.interface, now);
        if let Some(gateway) = &mut self.gateway {
            gateway.poll_queued(&mut self.sockets, now, self.device.rx.is_empty());
        }
        self.device.tx_limit = QUEUE.saturating_sub(self.ready.len());
        let now = Instant::from_millis(now.min(i64::MAX as u64) as i64);
        self.interface
            .poll(now, &mut self.device, &mut self.sockets);
        while let Some(frame) = self.device.tx.pop_front() {
            let frame = if let Some(gateway) = &mut self.gateway {
                gateway.egress(frame)
            } else {
                Some(frame)
            };
            if let Some(frame) = frame {
                let scope = self.forward.scope_for(&frame);
                if scope.is_none() && self.forward.reserved_output(&frame) {
                    continue;
                }
                self.ready.push_back(Output { frame, scope });
            }
        }
    }
    pub fn output(&mut self) -> Option<Vec<u8>> {
        self.take_output().map(|item| item.frame)
    }
    pub fn take_output(&mut self) -> Option<Output> {
        self.ready.pop_front()
    }
    pub fn scan_status(&self) -> &scanner::Status {
        self.scanner.status()
    }
    pub fn scan_revision(&self) -> u64 {
        self.scanner.revision()
    }
    pub fn start_scan(
        &mut self,
        request: scanner::Request,
        owner: crate::input_sessions::Principal,
        life: tokio_util::sync::CancellationToken,
    ) -> Result<tokio_util::sync::CancellationToken, AppError> {
        let token = self.scanner.start(request, owner, life)?;
        self.ready.retain(|item| {
            item.scope
                .as_ref()
                .is_none_or(|scope| scope.kind != scanner::Kind::Scan)
        });
        Ok(token)
    }
    pub fn stop_scan(&mut self) {
        self.scanner.stop();
        self.ready.retain(|item| {
            item.scope
                .as_ref()
                .is_none_or(|scope| scope.kind != scanner::Kind::Scan)
        });
    }
    pub fn output_len(&self) -> usize {
        self.ready.len() + self.device.tx.len()
    }
    pub fn dropped(&self) -> u64 {
        self.device.dropped + self.gateway.as_ref().map_or(0, |g| g.dropped)
    }
    pub fn flows(&self) -> usize {
        self.gateway.as_ref().map_or(0, Gateway::count)
    }
    pub fn flow_usage(&self) -> usize {
        self.budget.used()
    }
    pub fn forward_status(&self) -> forwarding::Status {
        self.forward.status()
    }
    pub fn start_forward(
        &mut self,
        bound: forwarding::Bound,
        owner: crate::input_sessions::Principal,
        life: tokio_util::sync::CancellationToken,
    ) -> Result<tokio_util::sync::CancellationToken, AppError> {
        self.forward.start(bound, owner, life)
    }
    pub fn stop_forward(&mut self) -> forwarding::Cleanup {
        let cleanup = self.forward.stop(&mut self.sockets);
        self.ready.retain(|item| {
            item.scope
                .as_ref()
                .is_none_or(|scope| scope.kind != scanner::Kind::Forward)
        });
        cleanup
    }
    pub async fn shutdown_io(&mut self) {
        self.stop_forward().finish().await;
        if let Some(gateway) = self.gateway.take() {
            gateway.shutdown().await;
        }
    }
    pub fn leases(&mut self, now: u64) -> Vec<dhcp::Lease> {
        self.dhcp.leases(now)
    }
}
