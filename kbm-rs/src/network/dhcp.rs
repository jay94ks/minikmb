//! Existing MiniKMB DHCPv4 profile. No relay; DNS is advertised only in host mode.
use super::Config;
use crate::model::AppError;
use serde::Serialize;
use std::{collections::HashMap, net::Ipv4Addr};
pub fn checksum(b: &[u8]) -> u16 {
    let mut sum: u32 = b
        .chunks(2)
        .map(|n| u32::from(u16::from_be_bytes([n[0], *n.get(1).unwrap_or(&0)])))
        .sum();
    while sum > 65535 {
        sum = (sum & 65535) + (sum >> 16);
    }
    !(sum as u16)
}
fn be(b: &[u8], n: usize) -> usize {
    u16::from_be_bytes(b[n..n + 2].try_into().unwrap()) as usize
}
fn put(b: &mut [u8], n: usize, value: usize) {
    b[n..n + 2].copy_from_slice(&(value as u16).to_be_bytes());
}
pub struct Server {
    config: Config,
    ip: Ipv4Addr,
    mac: [u8; 6],
    dns: bool,
    leases: HashMap<[u8; 6], Lease>,
}
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct Lease {
    pub ip: Ipv4Addr,
    pub mac: [u8; 6],
    pub expires_ms: u64,
    pub offered: bool,
}
pub enum Reply {
    Other,
    Consumed(Option<Vec<u8>>),
}
impl Server {
    pub fn new(config: Config, mac: [u8; 6], host: bool) -> Result<Self, AppError> {
        let ip = config.validate()?;
        Ok(Self {
            config,
            ip,
            mac,
            dns: host,
            leases: HashMap::new(),
        })
    }
    pub fn leases(&mut self, now: u64) -> Vec<Lease> {
        self.leases.retain(|_, l| l.expires_ms > now);
        let mut result: Vec<_> = self.leases.values().cloned().collect();
        result.sort_by_key(|l| l.ip);
        result
    }
    pub fn handle(&mut self, frame: &[u8], now: u64) -> Reply {
        if frame.len() < 42 || frame[12..14] != [8, 0] || frame[14] >> 4 != 4 {
            return Reply::Other;
        }
        let ihl = usize::from(frame[14] & 15) * 4;
        let total = be(frame, 16);
        if ihl < 20 || frame.len() < 14 + ihl + 8 || frame[23] != 17 {
            return Reply::Other;
        }
        let at = 14 + ihl;
        if be(frame, at) != 68 || be(frame, at + 2) != 67 {
            return Reply::Other;
        }
        Reply::Consumed(self.packet(frame, now, ihl, total, at))
    }
    fn packet(
        &mut self,
        frame: &[u8],
        now: u64,
        ihl: usize,
        total: usize,
        at: usize,
    ) -> Option<Vec<u8>> {
        if total < ihl + 248
            || total + 14 > frame.len()
            || be(frame, 20) & 0x3fff != 0
            || checksum(&frame[14..14 + ihl]) != 0
        {
            return None;
        }
        let length = be(frame, at + 4);
        if length < 248 || length > total - ihl {
            return None;
        }
        if be(frame, at + 6) != 0 {
            let mut pseudo = vec![0; 12 + length];
            pseudo[..8].copy_from_slice(&frame[26..34]);
            pseudo[9] = 17;
            put(&mut pseudo, 10, length);
            pseudo[12..].copy_from_slice(&frame[at..at + length]);
            if checksum(&pseudo) != 0 {
                return None;
            }
        }
        let p = &frame[at + 8..at + length];
        if p[..4] != [1, 1, 6, 0] || p[236..240] != [99, 130, 83, 99] || p[24..28] != [0; 4] {
            return None;
        }
        let mut options: HashMap<u8, &[u8]> = HashMap::new();
        let mut n = 240;
        while n < p.len() {
            let code = p[n];
            n += 1;
            if code == 255 {
                break;
            }
            if code == 0 {
                continue;
            }
            let size = usize::from(*p.get(n)?);
            n += 1;
            if n + size > p.len() {
                return None;
            }
            if options.insert(code, &p[n..n + size]).is_some() {
                return None;
            }
            n += size;
        }
        let kind = *options.get(&53)?.first()?;
        if options[&53].len() != 1 {
            return None;
        }
        let mac: [u8; 6] = p[28..34].try_into().unwrap();
        // A DHCP client MAC must be unicast and nonzero; no unbounded identities.
        if mac[0] & 1 != 0 || mac == [0; 6] {
            return None;
        }
        self.leases.retain(|_, l| l.expires_ms > now);
        if [4, 7].contains(&kind) {
            self.leases.remove(&mac);
            return None;
        }
        if ![1, 3, 8].contains(&kind) {
            return None;
        }
        if options.get(&54).is_some_and(|s| *s != self.ip.octets()) {
            return None;
        }
        if !self.leases.contains_key(&mac) && kind != 8 {
            let octet = (self.config.pool_start..=self.config.pool_end)
                .find(|last| !self.leases.values().any(|l| l.ip.octets()[3] == *last))?;
            let mut ip = self.ip.octets();
            ip[3] = octet;
            self.leases.insert(
                mac,
                Lease {
                    ip: ip.into(),
                    mac,
                    expires_ms: now.saturating_add(30000),
                    offered: true,
                },
            );
        }
        let requested = options.get(&50).copied().unwrap_or(&p[12..16]);
        let mismatch = kind == 3
            && self
                .leases
                .get(&mac)
                .is_none_or(|l| requested != l.ip.octets());
        let reply = if mismatch {
            6
        } else if kind == 1 {
            2
        } else {
            5
        };
        if kind == 3 && !mismatch {
            let l = self.leases.get_mut(&mac).unwrap();
            l.expires_ms = now.saturating_add(u64::from(self.config.lease_seconds) * 1000);
            l.offered = false;
        }
        let server = self.ip.octets();
        let mut opts = vec![53, 1, reply, 54, 4];
        opts.extend_from_slice(&server);
        opts.extend_from_slice(&[1, 4, 255, 255, 255, 0, 3, 4]);
        opts.extend_from_slice(&server);
        if kind != 8 && !mismatch {
            opts.extend_from_slice(&[51, 4]);
            opts.extend_from_slice(&self.config.lease_seconds.to_be_bytes());
        }
        if self.dns {
            opts.extend_from_slice(&[6, 4]);
            opts.extend_from_slice(&server);
        }
        opts.push(255);
        let mut body = vec![0; 300.max(240 + opts.len())];
        body[..44].copy_from_slice(&p[..44]);
        body[0] = 2;
        body[3] = 0;
        body[12..28].fill(0);
        if kind != 8 && !mismatch {
            body[16..20].copy_from_slice(&self.leases.get(&mac)?.ip.octets());
        }
        body[20..24].copy_from_slice(&server);
        body[236..240].copy_from_slice(&[99, 130, 83, 99]);
        body[240..240 + opts.len()].copy_from_slice(&opts);
        let mut out = vec![0; 42 + body.len()];
        let out_len = out.len();
        out[..6].fill(255);
        out[6..12].copy_from_slice(&self.mac);
        out[12..14].copy_from_slice(&[8, 0]);
        out[14] = 0x45;
        put(&mut out, 16, out_len - 14);
        out[22] = 64;
        out[23] = 17;
        out[26..30].copy_from_slice(&server);
        out[30..34].fill(255);
        let c = checksum(&out[14..34]);
        put(&mut out, 24, c as usize);
        put(&mut out, 34, 67);
        put(&mut out, 36, 68);
        put(&mut out, 38, body.len() + 8);
        out[42..].copy_from_slice(&body);
        Some(out)
    }
}
