//! management_network.inc ABI: exact fragments, full Ethernet CRC, absolute credit.
use crate::{model::AppError, protocol::Frame};
use std::time::{Duration, Instant};
pub const MIN: usize = 14;
pub const MAX: usize = 1518;
pub const FRAGMENT: usize = 1008;
pub const WINDOW: u64 = 32768;
pub const RENEW: u64 = 16384;
#[derive(Clone, Copy, Default)]
pub struct Watermark {
    pub sequence: u32,
    pub frame_id: u32,
}
impl Watermark {
    pub fn observe(&mut self, f: &Frame) {
        self.sequence = self.sequence.max(f.sequence);
        if f.payload.len() >= 4 {
            self.frame_id = self.frame_id.max(u32at(&f.payload, 0));
        }
    }
}
struct Partial {
    id: u32,
    total: usize,
    crc: u32,
    bytes: Vec<u8>,
    deadline: Instant,
}
pub struct Receiver {
    session: u64,
    sequence: u32,
    frame_id: u32,
    partial: Option<Partial>,
    pub received: u64,
    pub granted: u64,
    pub dropped: u64,
}
fn invalid() -> AppError {
    AppError::new(
        "NETWORK_PROTOCOL_ERROR",
        "NCM 수신 순서/credit을 확인하지 못했습니다. 링크를 다시 켜서 동기화하세요.",
    )
}
fn u16at(b: &[u8], at: usize) -> usize {
    u16::from_le_bytes(b[at..at + 2].try_into().unwrap()) as usize
}
fn u32at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}
pub fn credit(limit: u64) -> Vec<u8> {
    let mut b = vec![4, 0, 0, 0];
    b.extend_from_slice(&limit.to_le_bytes());
    b
}
pub fn credit_reply(b: &[u8]) -> Result<(u64, u64), AppError> {
    if b.len() != 16 {
        return Err(invalid());
    }
    let sent = u64::from_le_bytes(b[..8].try_into().unwrap());
    let limit = u64::from_le_bytes(b[8..].try_into().unwrap());
    if sent > limit || limit - sent > WINDOW {
        return Err(invalid());
    }
    Ok((sent, limit))
}
impl Receiver {
    pub fn resumed(
        session: u64,
        sent: u64,
        granted: u64,
        watermark: Watermark,
    ) -> Result<Self, AppError> {
        let mut rx = Self::synchronized(session, sent, granted)?;
        rx.sequence = watermark.sequence;
        rx.frame_id = watermark.frame_id;
        Ok(rx)
    }
    pub fn watermark(&self) -> Watermark {
        Watermark {
            sequence: self.sequence,
            frame_id: self.frame_id,
        }
    }
    /// Only after NET_SET(0) ACK + SYS_CREDIT(0) ACK and draining old events.
    pub fn synchronized(session: u64, sent: u64, granted: u64) -> Result<Self, AppError> {
        if session == 0 || sent > granted || granted - sent > WINDOW {
            return Err(invalid());
        }
        Ok(Self {
            session,
            sequence: 0,
            frame_id: 0,
            partial: None,
            received: sent,
            granted,
            dropped: 0,
        })
    }
    pub fn renewal(&mut self) -> Result<Option<u64>, AppError> {
        if self.received > self.granted {
            return Err(invalid());
        }
        if self.granted - self.received > RENEW {
            return Ok(None);
        }
        let limit = self.received.checked_add(WINDOW).ok_or_else(invalid)?;
        self.granted = limit;
        Ok(Some(limit))
    }
    pub fn expire(&mut self, now: Instant) {
        if self.partial.as_ref().is_some_and(|p| now >= p.deadline) {
            self.partial = None;
            self.dropped += 1;
        }
    }
    fn reject(&mut self) -> Result<Option<Vec<u8>>, AppError> {
        self.partial = None;
        self.dropped += 1;
        Ok(None)
    }
    pub fn feed(&mut self, f: &Frame, now: Instant) -> Result<Option<Vec<u8>>, AppError> {
        if f.session != self.session || f.kind != 4 || f.channel != 4 || f.opcode != 0x480 {
            return Ok(None);
        }
        if f.sequence == 0 {
            return Err(invalid());
        }
        // A duplicate event did not consume new MCU credit. Never count it twice.
        if f.sequence <= self.sequence {
            return Ok(None);
        }
        if self.sequence != 0 && self.sequence.checked_add(1) != Some(f.sequence) {
            self.partial = None;
            self.dropped += 1;
            return Err(invalid());
        }
        self.sequence = f.sequence;
        let b = &f.payload;
        let next = self
            .received
            .checked_add(b.len() as u64)
            .ok_or_else(invalid)?;
        if next > self.granted {
            return Err(invalid());
        }
        self.received = next;
        self.expire(now);
        if b.len() <= 16 || b.len() > 1024 {
            return self.reject();
        }
        let id = u32at(b, 0);
        let offset = u16at(b, 4);
        let total = u16at(b, 6);
        let crc = u32at(b, 8);
        if id == 0
            || !(MIN..=MAX).contains(&total)
            || offset >= total
            || b.len() - 16 != (total - offset).min(FRAGMENT)
            || b[12..16] != [0, 0, 0, 0]
        {
            return self.reject();
        }
        if offset == 0 && id > self.frame_id {
            if self.partial.take().is_some() {
                self.dropped += 1;
            }
            self.frame_id = id;
            self.partial = Some(Partial {
                id,
                total,
                crc,
                bytes: Vec::with_capacity(total),
                deadline: now + Duration::from_secs(1),
            });
        }
        let Some(p) = &mut self.partial else {
            return self.reject();
        };
        if p.id != id || p.total != total || p.crc != crc || p.bytes.len() != offset {
            return self.reject();
        }
        p.bytes.extend_from_slice(&b[16..]);
        if p.bytes.len() != total {
            return Ok(None);
        }
        let p = self.partial.take().unwrap();
        if crc32fast::hash(&p.bytes) != p.crc {
            return self.reject();
        }
        Ok(Some(p.bytes))
    }
}
/// Allocate/store the ID before sending the first fragment. Retain that counter
/// across OFF/ON in the same management session, even after an uncertain ACK.
pub fn fragments(id: u32, frame: &[u8]) -> Result<Vec<Vec<u8>>, AppError> {
    if id == 0 || !(MIN..=MAX).contains(&frame.len()) {
        return Err(AppError::new(
            "INVALID_ARGUMENTS",
            "Ethernet 프레임은14..1518바이트이며 ID는0일 수 없습니다.",
        ));
    }
    let crc = crc32fast::hash(frame);
    let mut result = Vec::with_capacity(2);
    for (n, data) in frame.chunks(FRAGMENT).enumerate() {
        let mut b = Vec::with_capacity(16 + data.len());
        b.extend_from_slice(&id.to_le_bytes());
        b.extend_from_slice(&((n * FRAGMENT) as u16).to_le_bytes());
        b.extend_from_slice(&(frame.len() as u16).to_le_bytes());
        b.extend_from_slice(&crc.to_le_bytes());
        b.extend_from_slice(&[0; 4]);
        b.extend_from_slice(data);
        result.push(b);
    }
    Ok(result)
}
/// Keep one instance per management session, across mode OFF/ON. An allocated
/// frame ID remains spent after cancellation, negative ACK or unknown completion.
pub struct Transmitter {
    session: u64,
    last: u32,
}
impl Transmitter {
    pub fn new(session: u64) -> Result<Self, AppError> {
        if session == 0 {
            return Err(AppError::new(
                "STALE_SESSION",
                "유효한 NCM 관리 세션이 필요합니다.",
            ));
        }
        Ok(Self { session, last: 0 })
    }
    pub fn prepare(&mut self, session: u64, frame: &[u8]) -> Result<Vec<Vec<u8>>, AppError> {
        if session != self.session {
            return Err(AppError::new(
                "STALE_SESSION",
                "NCM 프레임을 원래 세션에만 전송할 수 있습니다.",
            ));
        }
        let next = self.last.checked_add(1).ok_or_else(|| {
            AppError::new(
                "SESSION_EXHAUSTED",
                "NCM 프레임 ID를 소진했습니다. 관리 USB를 다시 연결하세요.",
            )
        })?;
        let parts = fragments(next, frame)?;
        self.last = next;
        Ok(parts)
    }
}
#[cfg(test)]
mod tx_tests {
    use super::*;
    #[test]
    fn ids_survive_unknown_completion_and_never_roll_over_or_cross_session() {
        let mut tx = Transmitter::new(7).unwrap();
        assert_eq!(u32at(&tx.prepare(7, &[0; 64]).unwrap()[0], 0), 1);
        // Simulate an unknown first-fragment ACK: no commit/rollback API exists.
        assert_eq!(u32at(&tx.prepare(7, &[0; 64]).unwrap()[0], 0), 2);
        assert!(tx.prepare(8, &[0; 64]).is_err());
        assert_eq!(tx.last, 2);
        assert!(tx.prepare(7, &[0; 13]).is_err());
        assert_eq!(tx.last, 2);
        tx.last = u32::MAX - 1;
        assert_eq!(u32at(&tx.prepare(7, &[0; 64]).unwrap()[0], 0), u32::MAX);
        assert!(tx.prepare(7, &[0; 64]).is_err());
        assert_eq!(tx.last, u32::MAX);
    }
}
#[derive(Clone, Debug, Default, serde::Serialize, schemars::JsonSchema)]
pub struct McuStats {
    pub rx_dropped: u32,
    pub tx_dropped: u32,
    pub credit_dropped: u32,
    pub enabled: bool,
    pub link_up: bool,
}
impl McuStats {
    pub fn decode(b: &[u8]) -> Result<Self, AppError> {
        if b.len() != 16 || b[12] > 1 || b[13] > 1 || b[14..] != [0, 0] || b[13] > b[12] {
            return Err(invalid());
        }
        Ok(Self {
            rx_dropped: u32at(b, 0),
            tx_dropped: u32at(b, 4),
            credit_dropped: u32at(b, 8),
            enabled: b[12] != 0,
            link_up: b[13] != 0,
        })
    }
}
