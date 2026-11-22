//! Explicit, selected-NCM-only SYN scanning. No host socket or OS network changes.
use super::{
    ipv4::{self, Key},
    Config,
};
use crate::{input_sessions::Principal, model::AppError};
use serde::{Deserialize, Serialize};
use std::net::Ipv4Addr;
use tokio_util::sync::CancellationToken;
#[cfg(test)]
mod tests;
pub const CONCURRENCY: usize = 8;
const SOURCE: u16 = 41000;
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub target: String,
    pub start: u16,
    pub end: u16,
    pub timeout_ms: u32,
}
impl Request {
    pub fn validate(&self, config: &Config) -> Result<Ipv4Addr, AppError> {
        let ip = self.target.parse::<Ipv4Addr>().map_err(|_| invalid())?;
        if self.target != ip.to_string()
            || self.start == 0
            || self.end < self.start
            || !(250..=5000).contains(&self.timeout_ms)
        {
            return Err(invalid());
        }
        config.target(ip)?;
        Ok(ip)
    }
}
fn invalid() -> AppError {
    AppError::new(
        "INVALID_ARGUMENTS",
        "스캔 타깃은 NCM /24 주소, 포트는1..65535, 제한 시간은250..5000ms여야 합니다.",
    )
}
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct Status {
    pub running: bool,
    pub phase: String,
    pub target: String,
    pub start: u16,
    pub end: u16,
    pub timeout_ms: u32,
    pub completed: u32,
    pub total: u32,
    pub open: Vec<u16>,
    pub closed: u32,
    pub filtered: u32,
    pub pending: usize,
    pub error: Option<AppError>,
}
impl Default for Status {
    fn default() -> Self {
        Self {
            running: false,
            phase: "idle".into(),
            target: String::new(),
            start: 1,
            end: 1024,
            timeout_ms: 1000,
            completed: 0,
            total: 0,
            open: vec![],
            closed: 0,
            filtered: 0,
            pending: 0,
            error: None,
        }
    }
}
#[derive(Clone)]
pub struct Scope {
    pub life: CancellationToken,
    pub owner: Principal,
    pub kind: Kind,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Scan,
    Forward,
}
impl Scope {
    pub fn cancelled(&self) -> bool {
        self.life.is_cancelled()
            || self.owner.cancelled.is_cancelled()
            || self.owner.ended.is_cancelled()
    }
}
pub struct Output {
    pub frame: Vec<u8>,
    pub scope: Option<Scope>,
}
impl From<Vec<u8>> for Output {
    fn from(frame: Vec<u8>) -> Self {
        Self { frame, scope: None }
    }
}
struct Probe {
    port: u16,
    sequence: u32,
    sent: u64,
    reset: Option<Vec<u8>>,
}
pub struct Scanner {
    config: Config,
    server: Ipv4Addr,
    mac: [u8; 6],
    target: Ipv4Addr,
    dest_mac: Option<[u8; 6]>,
    scope: Option<Scope>,
    probes: Vec<Option<Probe>>,
    next: u32,
    sequence: u32,
    arp_sent: Option<u64>,
    status: Status,
    revision: u64,
}
impl Drop for Scanner {
    fn drop(&mut self) {
        if let Some(scope) = &self.scope {
            scope.life.cancel();
        }
    }
}
impl Scanner {
    pub fn new(config: Config, mac: [u8; 6]) -> Result<Self, AppError> {
        Ok(Self {
            server: config.validate()?,
            config,
            mac,
            target: Ipv4Addr::UNSPECIFIED,
            dest_mac: None,
            scope: None,
            probes: (0..CONCURRENCY).map(|_| None).collect(),
            next: 0,
            sequence: 0,
            arp_sent: None,
            status: Default::default(),
            revision: 0,
        })
    }
    pub fn status(&self) -> &Status {
        &self.status
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    fn changed(&mut self) {
        self.status.pending = self
            .probes
            .iter()
            .flatten()
            .filter(|p| p.reset.is_none())
            .count();
        self.revision = self.revision.wrapping_add(1);
    }
    pub fn start(
        &mut self,
        request: Request,
        owner: Principal,
        life: CancellationToken,
    ) -> Result<CancellationToken, AppError> {
        let target = request.validate(&self.config)?;
        if owner.cancelled.is_cancelled() || owner.ended.is_cancelled() || life.is_cancelled() {
            return Err(AppError::new(
                "OPERATION_CANCELLED",
                "스캔 요청 연결이 종료됐습니다.",
            ));
        }
        let mut seed = [0; 4];
        getrandom::fill(&mut seed)
            .map_err(|_| AppError::new("RANDOM_FAILED", "스캔 순서 번호를 준비하지 못했습니다."))?;
        self.stop();
        self.sequence = u32::from_le_bytes(seed);
        self.target = target;
        self.dest_mac = None;
        self.arp_sent = None;
        self.next = u32::from(request.start);
        self.status = Status {
            running: true,
            phase: "resolving".into(),
            target: request.target,
            start: request.start,
            end: request.end,
            timeout_ms: request.timeout_ms,
            total: u32::from(request.end) - u32::from(request.start) + 1,
            ..Default::default()
        };
        let token = life.child_token();
        self.scope = Some(Scope {
            life: token.clone(),
            owner,
            kind: Kind::Scan,
        });
        self.changed();
        Ok(token)
    }
    pub fn stop(&mut self) {
        if let Some(scope) = self.scope.take() {
            scope.life.cancel();
        }
        for probe in &mut self.probes {
            *probe = None;
        }
        if self.status.running {
            self.status.running = false;
            self.status.phase = "stopped".into();
        }
        self.changed();
    }
    fn output(&self, frame: Vec<u8>) -> Output {
        Output {
            frame,
            scope: self.scope.clone(),
        }
    }
    pub fn poll(&mut self, now: u64, space: usize) -> Vec<Output> {
        if !self.status.running {
            return vec![];
        }
        if self.scope.as_ref().is_none_or(Scope::cancelled) {
            self.stop();
            self.status.phase = "cancelled".into();
            return vec![];
        }
        let mut out = vec![];
        if self.dest_mac.is_none() {
            if let Some(sent) = self.arp_sent {
                if now.saturating_sub(sent) >= 2000 {
                    self.status.running = false;
                    self.status.phase = "failed".into();
                    self.status.error = Some(AppError::new(
                        "SCAN_NO_ARP",
                        "타깃의 ARP 응답이 없습니다. IP 주소를 확인하세요.",
                    ));
                    if let Some(scope) = &self.scope {
                        scope.life.cancel();
                    }
                    self.changed();
                }
            } else if space > 0 {
                let mut b = vec![0; 42];
                b[..6].fill(255);
                b[6..12].copy_from_slice(&self.mac);
                b[12..22].copy_from_slice(&[8, 6, 0, 1, 8, 0, 6, 4, 0, 1]);
                b[22..28].copy_from_slice(&self.mac);
                b[28..32].copy_from_slice(&self.server.octets());
                b[38..42].copy_from_slice(&self.target.octets());
                self.arp_sent = Some(now);
                out.push(self.output(b));
            }
            return out;
        }
        for slot in 0..CONCURRENCY {
            if self.probes[slot].as_ref().is_some_and(|p| {
                p.reset.is_none() && now.saturating_sub(p.sent) >= u64::from(self.status.timeout_ms)
            }) {
                self.probes[slot] = None;
                self.status.completed += 1;
                self.status.filtered += 1;
                self.changed();
            }
            if out.len() >= space {
                continue;
            }
            if let Some(frame) = self.probes[slot].as_mut().and_then(|p| p.reset.take()) {
                out.push(self.output(frame));
                self.probes[slot] = None;
            }
            if out.len() >= space {
                continue;
            }
            if self.probes[slot].is_none() && self.next <= u32::from(self.status.end) {
                let port = self.next as u16;
                self.next += 1;
                self.sequence = self.sequence.wrapping_add(1);
                let sequence = self.sequence;
                let key = Key {
                    protocol: 6,
                    client: self.server,
                    client_port: SOURCE + slot as u16,
                    remote: self.target,
                    remote_port: port,
                };
                let frame = ipv4::tcp_frame(self.mac, self.dest_mac.unwrap(), key, sequence, 0, 2);
                self.probes[slot] = Some(Probe {
                    port,
                    sequence,
                    sent: now,
                    reset: None,
                });
                out.push(self.output(frame));
                self.changed();
            }
        }
        if self.next > u32::from(self.status.end) && self.probes.iter().all(Option::is_none) {
            self.status.running = false;
            self.status.phase = "completed".into();
            self.changed();
        }
        out
    }
    pub fn receive(&mut self, frame: &[u8]) -> bool {
        if !self.status.running || self.scope.as_ref().is_none_or(Scope::cancelled) {
            return false;
        }
        if self.dest_mac.is_none()
            && self.arp_sent.is_some()
            && frame.len() >= 42
            && frame[..6] == self.mac
            && frame[12..22] == [8, 6, 0, 1, 8, 0, 6, 4, 0, 2]
            && frame[6..12] == frame[22..28]
            && frame[22] & 1 == 0
            && frame[22..28] != [0; 6]
            && frame[28..32] == self.target.octets()
            && frame[32..38] == self.mac
            && frame[38..42] == self.server.octets()
        {
            self.dest_mac = Some(frame[22..28].try_into().unwrap());
            self.status.phase = "probing".into();
            self.changed();
            return false;
        }
        let Some(p) = ipv4::packet(frame) else {
            return false;
        };
        if p.key.protocol != 6
            || p.key.client != self.target
            || p.key.remote != self.server
            || frame[..6] != self.mac
            || self.dest_mac.is_none_or(|mac| frame[6..12] != mac)
            || !p.valid()
        {
            return false;
        }
        let Some(slot) = p
            .key
            .remote_port
            .checked_sub(SOURCE)
            .map(usize::from)
            .filter(|s| *s < CONCURRENCY)
        else {
            return false;
        };
        let Some(probe) = &self.probes[slot] else {
            return false;
        };
        if probe.port != p.key.client_port || probe.reset.is_some() {
            return false;
        }
        let ack = u32::from_be_bytes(frame[p.offset + 8..p.offset + 12].try_into().unwrap());
        let flags = frame[p.offset + 13];
        if flags & 0x10 == 0 || ack != probe.sequence.wrapping_add(1) {
            return false;
        }
        if flags & 0x16 == 0x12 {
            let seq = u32::from_be_bytes(frame[p.offset + 4..p.offset + 8].try_into().unwrap());
            let key = Key {
                protocol: 6,
                client: self.server,
                client_port: p.key.remote_port,
                remote: self.target,
                remote_port: p.key.client_port,
            };
            let reset = ipv4::tcp_frame(
                self.mac,
                self.dest_mac.unwrap(),
                key,
                ack,
                seq.wrapping_add(1),
                0x14,
            );
            self.status.open.push(probe.port);
            self.status.open.sort_unstable();
            self.probes[slot].as_mut().unwrap().reset = Some(reset);
        } else if flags & 4 != 0 {
            self.status.closed += 1;
            self.probes[slot] = None;
        } else {
            return false;
        }
        self.status.completed += 1;
        self.changed();
        true
    }
}
