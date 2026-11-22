//! Hardware-independent request/response and HID completion state machine.
//! Each wire channel has its own in-flight request; STORAGE DATA has a four
//! request window which advances only over a contiguous acknowledged prefix.
use crate::{model::AppError, protocol::Frame};
use std::collections::{HashMap, VecDeque};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Completion {
    Ack,
    HidDone,
}
struct Pending {
    id: u64,
    sequence: u32,
    opcode: u16,
    deadline: u64,
    completion: Completion,
    response: Option<Result<Vec<u8>, AppError>>,
}
struct HidWait {
    id: u64,
    deadline: u64,
    body: Vec<u8>,
}
pub enum Effect {
    Done(u64, Result<Vec<u8>, AppError>),
    Event(Frame),
}
pub struct Machine {
    session: u64,
    next: [u32; 6],
    pending: [VecDeque<Pending>; 6],
    hid: HashMap<u32, HidWait>,
    early: VecDeque<(u32, u16)>,
    event_sequence: [u32; 6],
    pub event_gaps: u64,
    pub ignored: u64,
    pub fatal: Option<AppError>,
    pub fault_request: Option<(u8, u16, u32)>,
    pub last_request: Option<(u8, u16, u32)>,
}
impl Machine {
    /// An overlapped write may still be pending in the driver. ACK time starts
    /// only after the complete wire frame is written, never on I/O readiness.
    pub fn writing(&mut self, ch: u8, sequence: u32) {
        if let Some(p) = self.pending[ch as usize]
            .iter_mut()
            .find(|p| p.sequence == sequence)
        {
            p.deadline = u64::MAX;
        }
    }
    pub fn written(&mut self, ch: u8, sequence: u32, now: u64) {
        if let Some(p) = self.pending[ch as usize]
            .iter_mut()
            .find(|p| p.sequence == sequence)
        {
            p.deadline = now.saturating_add(1000);
        }
    }
    pub fn next_deadline(&self) -> Option<u64> {
        self.pending
            .iter()
            .filter_map(|q| q.front().map(|p| p.deadline))
            .chain(self.hid.values().map(|h| h.deadline))
            .filter(|d| *d != u64::MAX)
            .min()
    }
    pub fn new(session: u64, next: [u32; 6]) -> Self {
        Self {
            session,
            next,
            pending: std::array::from_fn(|_| VecDeque::new()),
            hid: HashMap::new(),
            early: VecDeque::new(),
            event_sequence: [0; 6],
            event_gaps: 0,
            ignored: 0,
            fatal: None,
            fault_request: None,
            last_request: None,
        }
    }
    pub fn can_send(&self, ch: u8, op: u16, completion: Completion) -> bool {
        if self.fatal.is_some()
            || ch > 5
            || (completion == Completion::HidDone && self.hid.len() >= 64)
        {
            return false;
        }
        let p = &self.pending[ch as usize];
        p.is_empty()
            || (ch == 3 && op == 0x302 && p.len() < 4 && p.iter().all(|p| p.opcode == 0x302))
    }
    pub fn prepare(
        &mut self,
        id: u64,
        ch: u8,
        op: u16,
        payload: Vec<u8>,
        completion: Completion,
        now: u64,
    ) -> Result<Frame, AppError> {
        if payload.len() > 1024 || !self.can_send(ch, op, completion) {
            return Err(AppError::new("QUEUE_FULL", "관리 요청 대기열이 찼습니다."));
        }
        if completion == Completion::HidDone && (ch != 1 || !(0x101..=0x107).contains(&op)) {
            return Err(AppError::new(
                "INVALID_COMMAND",
                "HID 완료 대기 명령이 아닙니다.",
            ));
        }
        let seq = self.next[ch as usize];
        if seq == 0 || seq >= 0xffff_fffe {
            return Err(AppError::new(
                "SESSION_EXHAUSTED",
                "MCU 세션을 다시 연결해야 합니다.",
            ));
        }
        self.next[ch as usize] += 1;
        self.last_request = Some((ch, op, seq));
        self.pending[ch as usize].push_back(Pending {
            id,
            sequence: seq,
            opcode: op,
            deadline: now + 1000,
            completion,
            response: None,
        });
        Ok(Frame {
            kind: 1,
            channel: ch,
            opcode: op,
            session: self.session,
            sequence: seq,
            payload,
        })
    }
    pub fn feed(&mut self, f: Frame, now: u64) -> Vec<Effect> {
        let mut out = Vec::new();
        if f.session != self.session || f.channel > 5 {
            self.ignored += 1;
            return out;
        }
        if f.kind == 2 {
            let Some(at) = self.pending[f.channel as usize]
                .iter()
                .position(|p| p.sequence == f.sequence && p.opcode == f.opcode)
            else {
                self.ignored += 1;
                return out;
            };
            let result = response_body(&f.payload);
            if result.as_ref().err().is_some_and(|e| {
                e.code == "INVALID_RESPONSE" || e.status.is_some_and(|s| (5..=8).contains(&s))
            }) {
                self.fault_request = Some((f.channel, f.opcode, f.sequence));
                self.fail(
                    AppError::uncertain(
                        "SESSION_INVALID",
                        "관리 응답/요청 순서가 일치하지 않습니다. 자동 재전송하지 않습니다.",
                    ),
                    &mut out,
                );
                return out;
            }
            let p = &mut self.pending[f.channel as usize][at];
            if p.response.is_some() {
                self.ignored += 1;
                return out;
            }
            p.response = Some(result);
            while self.pending[f.channel as usize]
                .front()
                .is_some_and(|p| p.response.is_some())
            {
                let mut p = self.pending[f.channel as usize].pop_front().unwrap();
                let result = p.response.take().unwrap();
                if p.completion == Completion::HidDone {
                    match result {
                        Ok(body) if body.len() == 4 && body != [0, 0, 0, 0] => {
                            let ticket = u32::from_le_bytes(body.as_slice().try_into().unwrap());
                            if let Some(index) = self.early.iter().position(|(t, _)| *t == ticket) {
                                let (_, status) = self.early.remove(index).unwrap();
                                out.push(Effect::Done(p.id, hid_status(status).map(|_| body)));
                            } else if let std::collections::hash_map::Entry::Vacant(entry) =
                                self.hid.entry(ticket)
                            {
                                entry.insert(HidWait {
                                    id: p.id,
                                    deadline: now + 1000,
                                    body,
                                });
                            } else {
                                out.push(Effect::Done(
                                    p.id,
                                    Err(AppError::uncertain(
                                        "INVALID_HID_TICKET",
                                        "HID 완료 번호가 중복됐습니다.",
                                    )),
                                ));
                            }
                        }
                        Ok(_) => out.push(Effect::Done(
                            p.id,
                            Err(AppError::uncertain(
                                "INVALID_HID_TICKET",
                                "HID 완료 번호 응답이 잘못됐습니다.",
                            )),
                        )),
                        Err(e) => out.push(Effect::Done(p.id, Err(e))),
                    }
                } else {
                    out.push(Effect::Done(p.id, result));
                }
            }
        } else if f.kind == 3 || f.kind == 4 {
            let previous = self.event_sequence[f.channel as usize];
            if f.sequence == 0 || f.sequence <= previous {
                self.ignored += 1;
                return out;
            }
            if f.sequence != previous + 1 {
                self.event_gaps += u64::from(f.sequence - previous - 1);
            }
            self.event_sequence[f.channel as usize] = f.sequence;
            if f.kind == 3
                && f.channel == 1
                && f.opcode == 0x180
                && f.payload.len() == 8
                && f.payload[6..] == [0, 0]
            {
                let ticket = u32::from_le_bytes(f.payload[..4].try_into().unwrap());
                let status = u16::from_le_bytes(f.payload[4..6].try_into().unwrap());
                if let Some(wait) = self.hid.remove(&ticket) {
                    out.push(Effect::Done(wait.id, hid_status(status).map(|_| wait.body)));
                } else {
                    if self.early.len() == 64 {
                        self.early.pop_front();
                    }
                    self.early.push_back((ticket, status));
                }
            }
            out.push(Effect::Event(f));
        } else {
            self.ignored += 1;
        }
        out
    }
    pub fn tick(&mut self, now: u64) -> Vec<Effect> {
        let mut out = Vec::new();
        if let Some((ch, p)) = self
            .pending
            .iter()
            .enumerate()
            .find_map(|(ch, q)| q.front().filter(|p| now >= p.deadline).map(|p| (ch, p)))
        {
            self.fault_request = Some((ch as u8, p.opcode, p.sequence));
            self.fail(
                AppError::uncertain(
                    "RESPONSE_TIMEOUT",
                    "MCU 응답 시간이 초과됐습니다. 입력을 자동 재전송하지 않습니다.",
                ),
                &mut out,
            );
        }
        let expired: Vec<_> = self
            .hid
            .iter()
            .filter(|(_, w)| now >= w.deadline)
            .map(|(t, _)| *t)
            .collect();
        for t in expired {
            if let Some(w) = self.hid.remove(&t) {
                out.push(Effect::Done(
                    w.id,
                    Err(AppError::uncertain(
                        "HID_UNCONFIRMED",
                        "HID 전송 완료를 확인하지 못했습니다.",
                    )),
                ));
            }
        }
        out
    }
    pub fn disconnect(&mut self) -> Vec<Effect> {
        let mut out = Vec::new();
        self.fail(
            AppError::uncertain("DEVICE_DISCONNECTED", "MCU 연결이 끊겼습니다."),
            &mut out,
        );
        out
    }
    fn fail(&mut self, error: AppError, out: &mut Vec<Effect>) {
        self.fatal = Some(error.clone());
        for queue in &mut self.pending {
            for p in queue.drain(..) {
                out.push(Effect::Done(p.id, Err(error.clone())));
            }
        }
        for (_, w) in self.hid.drain() {
            out.push(Effect::Done(w.id, Err(error.clone())));
        }
        self.early.clear();
    }
}
fn hid_status(status: u16) -> Result<(), AppError> {
    if status == 0 {
        Ok(())
    } else {
        let mut e = AppError::mcu(status);
        e.code = "HID_FAILED";
        e.uncertain = true;
        Err(e)
    }
}
pub fn response_body(payload: &[u8]) -> Result<Vec<u8>, AppError> {
    if payload.len() < 8 || payload[2..4] != [0, 0] {
        return Err(AppError::new(
            "INVALID_RESPONSE",
            "관리 응답 헤더가 잘못됐습니다.",
        ));
    }
    let status = u16::from_le_bytes([payload[0], payload[1]]);
    if status != 0 {
        if payload.len() != 8 {
            return Err(AppError::new(
                "INVALID_RESPONSE",
                "오류 응답 크기가 잘못됐습니다.",
            ));
        }
        return Err(AppError::mcu(status));
    }
    if payload.len() > 264 || payload[4..8] != [0, 0, 0, 0] {
        return Err(AppError::new(
            "INVALID_RESPONSE",
            "관리 응답 크기/예약 값이 잘못됐습니다.",
        ));
    }
    Ok(payload[8..].to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pending_overlapped_write_does_not_expire_ack_before_wire_completion() {
        let mut m = Machine::new(42, [1; 6]);
        let f = m
            .prepare(1, 4, 0x402, vec![0; 1024], Completion::Ack, 0)
            .unwrap();
        m.writing(f.channel, f.sequence);
        assert!(m.tick(8000).is_empty());
        assert!(m.fatal.is_none());
        assert_eq!(m.next_deadline(), None);
        m.written(f.channel, f.sequence, 8000);
        assert_eq!(m.next_deadline(), Some(9000));
        assert!(m.tick(8999).is_empty());
        assert!(!m.tick(9000).is_empty());
        assert_eq!(m.fault_request, Some((4, 0x402, 1)));
    }
    #[test]
    fn ack_received_before_completion_observation_is_not_replayed_or_rearmed() {
        let mut m = Machine::new(42, [1; 6]);
        let f = m
            .prepare(1, 4, 0x402, vec![0; 1024], Completion::Ack, 0)
            .unwrap();
        m.writing(f.channel, f.sequence);
        assert!(matches!(
            &m.feed(response(&f, &[]), 50)[0],
            Effect::Done(1, Ok(_))
        ));
        m.written(f.channel, f.sequence, 51);
        assert_eq!(m.next_deadline(), None);
        assert!(m.tick(9999).is_empty());
    }
    fn response(request: &Frame, body: &[u8]) -> Frame {
        Frame {
            kind: 2,
            payload: [&[0; 8][..], body].concat(),
            ..request.clone()
        }
    }
    fn done(ticket: u32, sequence: u32) -> Frame {
        Frame {
            kind: 3,
            channel: 1,
            opcode: 0x180,
            session: 42,
            sequence,
            payload: [ticket.to_le_bytes().as_slice(), &[0; 4]].concat(),
        }
    }
    #[test]
    fn storage_holes_hold_window_without_blocking_hid_or_system() {
        let mut m = Machine::new(42, [1; 6]);
        let storage: Vec<_> = (0..4)
            .map(|id| {
                m.prepare(id, 3, 0x302, vec![id as u8], Completion::Ack, 0)
                    .unwrap()
            })
            .collect();
        assert!(!m.can_send(3, 0x302, Completion::Ack));
        assert!(!m.can_send(3, 0x303, Completion::Ack));
        assert!(m.feed(response(&storage[1], &[]), 1).is_empty());
        assert!(!m.can_send(3, 0x302, Completion::Ack));
        let key = m
            .prepare(99, 1, 0x101, vec![4, 0], Completion::HidDone, 2)
            .unwrap();
        let ping = m
            .prepare(100, 0, 2, vec![0; 8], Completion::Ack, 2)
            .unwrap();
        assert!(m.feed(response(&key, &7u32.to_le_bytes()), 3).is_empty());
        assert!(matches!(&m.feed(done(7, 1), 4)[0], Effect::Done(99, Ok(_))));
        assert!(matches!(
            &m.feed(response(&ping, &[0; 8]), 5)[0],
            Effect::Done(100, Ok(_))
        ));
        let ack = m.feed(response(&storage[0], &[]), 6);
        assert_eq!(ack.len(), 2);
        assert!(matches!(ack[0], Effect::Done(0, Ok(_))));
        assert!(matches!(ack[1], Effect::Done(1, Ok(_))));
        assert!(m.can_send(3, 0x302, Completion::Ack));
        assert!(!m.can_send(3, 0x303, Completion::Ack));
    }
    #[test]
    fn early_done_busy_and_timeout_never_repeat_down() {
        let mut m = Machine::new(42, [1; 6]);
        let key = m
            .prepare(1, 1, 0x101, vec![4, 0], Completion::HidDone, 0)
            .unwrap();
        assert!(matches!(
            m.feed(done(9, 1), 0).as_slice(),
            [Effect::Event(_)]
        ));
        assert!(matches!(
            &m.feed(response(&key, &9u32.to_le_bytes()), 1)[0],
            Effect::Done(1, Ok(_))
        ));
        let key = m
            .prepare(2, 1, 0x101, vec![5, 0], Completion::HidDone, 2)
            .unwrap();
        assert_eq!(key.sequence, 2);
        let mut busy = response(&key, &[]);
        busy.payload[0] = 4;
        assert!(
            matches!(&m.feed(busy,3)[0],Effect::Done(2,Err(e)) if e.status==Some(4)&&!e.uncertain)
        );
        let key = m
            .prepare(3, 1, 0x101, vec![5, 0], Completion::HidDone, 4)
            .unwrap();
        assert_eq!(key.sequence, 3);
        let expired = m.tick(1004);
        assert!(matches!(&expired[0],Effect::Done(3,Err(e)) if e.uncertain));
        assert!(m.fatal.is_some());
        assert!(!m.can_send(1, 0x101, Completion::HidDone));
    }
    #[test]
    fn wrong_session_and_duplicate_events_cannot_complete_current_input() {
        let mut m = Machine::new(42, [1; 6]);
        let request = m
            .prepare(1, 1, 0x101, vec![4, 0], Completion::HidDone, 0)
            .unwrap();
        let mut old = response(&request, &8u32.to_le_bytes());
        old.session = 41;
        assert!(m.feed(old, 1).is_empty());
        assert!(m
            .feed(response(&request, &8u32.to_le_bytes()), 2)
            .is_empty());
        assert!(m
            .feed(done(7, 1), 3)
            .iter()
            .all(|e| !matches!(e, Effect::Done(..))));
        assert!(m.feed(done(8, 1), 4).is_empty());
        let timeout = m.tick(1002);
        assert!(matches!(&timeout[0],Effect::Done(1,Err(e)) if e.code=="HID_UNCONFIRMED"));
        assert!(m.fatal.is_none()); // The original session may still accept RELEASE_ALL.
    }
}
