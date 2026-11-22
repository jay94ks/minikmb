use crate::{
    model::{now_ms, Channel, Inventory, Uid},
    protocol::{Decoder, Frame},
    topology,
};
use serialport::SerialPortType;
use std::{
    collections::{HashMap, VecDeque},
    io::{self, Read, Write},
    time::{Duration, Instant},
};

pub struct Session {
    pub(crate) port: crate::serial_io::Port,
    pub(crate) decoder: Decoder,
    pub(crate) frames: VecDeque<Frame>,
    pub(crate) session: u64,
    pub(crate) sequence: [u32; 6],
    pub channel: Channel,
}
impl Session {
    fn reply(&mut self, ch: u8, op: u16, seq: u32, hello: bool) -> io::Result<Frame> {
        let deadline = Instant::now() + Duration::from_secs(1);
        let mut bytes = [0u8; 4096];
        loop {
            while let Some(f) = self.frames.pop_front() {
                if f.kind == 2
                    && f.channel == ch
                    && f.opcode == op
                    && f.sequence == seq
                    && (hello || f.session == self.session)
                {
                    return Ok(f);
                }
            }
            if Instant::now() > deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "management response timeout",
                ));
            }
            match self.port.read(&mut bytes) {
                Ok(n) => self.frames.extend(self.decoder.feed(&bytes[..n])),
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                    ) => {}
                Err(e) => return Err(e),
            }
        }
    }
    fn body(frame: &Frame) -> io::Result<Vec<u8>> {
        if frame.payload.len() < 8 || frame.payload[2..4] != [0, 0] {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "response prefix",
            ));
        }
        let status = u16::from_le_bytes([frame.payload[0], frame.payload[1]]);
        if status != 0 {
            return Err(io::Error::other(format!("MCU status {status}")));
        }
        Ok(frame.payload[8..].to_vec())
    }
    pub fn command(&mut self, ch: u8, op: u16, payload: &[u8]) -> io::Result<Vec<u8>> {
        if ch > 5 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "channel bounds",
            ));
        }
        let seq = self.sequence[ch as usize];
        if seq >= 0xffff_fffe {
            return Err(io::Error::other("session renewal required"));
        }
        let frame = Frame {
            kind: 1,
            channel: ch,
            opcode: op,
            session: self.session,
            sequence: seq,
            payload: payload.to_vec(),
        };
        self.port.write_all(&frame.encode()?)?;
        let reply = self.reply(ch, op, seq, false)?;
        if reply.payload.len() >= 2
            && matches!(
                u16::from_le_bytes([reply.payload[0], reply.payload[1]]),
                5..=8
            )
        {
            return Err(io::Error::other("session/sequence mismatch"));
        }
        self.sequence[ch as usize] += 1;
        Self::body(&reply)
    }
    pub fn open(path: &str) -> io::Result<Self> {
        let port = crate::serial_io::Port::open(path)?;
        let mut s = Self {
            port,
            decoder: Decoder::default(),
            frames: VecDeque::new(),
            session: 0,
            sequence: [1; 6],
            channel: Channel {
                uid: Uid::from_bytes(&[0; 12]).unwrap(),
                name: String::new(),
                port: path.into(),
                connected: false,
                features: 0,
                target_state: 0,
                firmware: [0; 3],
                board_id: String::new(),
                usb_port: 0,
                slot: None,
                capture_id: None,
                pairing_error: String::new(),
                observed_at: 0,
            },
        };
        let mut nonce = [0u8; 8];
        getrandom::fill(&mut nonce).map_err(|e| io::Error::other(e.to_string()))?;
        let mut payload = vec![1, 1, 0, 0];
        payload.extend_from_slice(&nonce);
        let hello = Frame {
            kind: 1,
            channel: 0,
            opcode: 1,
            session: 0,
            sequence: 1,
            payload,
        };
        s.port.write_all(&[0])?;
        s.port.write_all(&hello.encode()?)?;
        let reply = s.reply(0, 1, 1, true)?;
        let body = Self::body(&reply)?;
        if reply.session == 0 || body.len() != 40 || body[..8] != nonce {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "HELLO nonce/session",
            ));
        }
        s.session = reply.session;
        s.channel.uid = Uid::from_bytes(&body[8..20]).map_err(io::Error::other)?;
        if s.command(0, 3, &[])?.as_slice() != body {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "INFO mismatch"));
        }
        s.channel.features = u32::from_le_bytes(body[28..32].try_into().unwrap());
        s.channel.firmware =
            std::array::from_fn(|n| u16::from_le_bytes([body[20 + n * 2], body[21 + n * 2]]));
        s.poll()?;
        Ok(s)
    }
    pub fn poll(&mut self) -> io::Result<()> {
        let body = self.command(0, 4, &[])?;
        if body.len() != 64 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "STATUS length"));
        }
        self.channel.target_state = body[12];
        self.channel.connected = true;
        self.channel.observed_at = now_ms();
        Ok(())
    }
    pub fn read_name(&mut self) {
        match self.command(3, 0x31e, &[]) {
            Ok(b)
                if b.len() >= 4
                    && b.len() == 4 + u16::from_le_bytes([b[0], b[1]]) as usize
                    && b[2..4] == [0, 0] =>
            {
                if let Ok(name) = std::str::from_utf8(&b[4..]) {
                    self.channel.name = name.into();
                }
            }
            _ => {
                self.channel.name.clear();
            }
        }
    }
}
#[derive(Default)]
pub struct DeviceManager {
    sessions: HashMap<String, Session>,
    revision: u64,
}
impl DeviceManager {
    pub fn refresh(&mut self, rescan: bool) -> Inventory {
        let mut failures = Vec::new();
        let ports = match serialport::available_ports() {
            Ok(p) => p,
            Err(e) => {
                return Inventory {
                    error: format!("관리 USB 검색 실패: {e}"),
                    ..Inventory::default()
                }
            }
        };
        let allowed:Vec<_>=ports.into_iter().filter(|p|matches!(&p.port_type,SerialPortType::UsbPort(u) if u.vid==0xcafe&&matches!(u.pid,0x4001|0x4002|0x4004))).collect();
        self.sessions
            .retain(|path, _| allowed.iter().any(|p| p.port_name == *path));
        for port in allowed {
            if let Some(session) = self.sessions.get_mut(&port.port_name) {
                if let Err(e) = session.poll() {
                    failures.push(format!("{}: {e}", port.port_name));
                    self.sessions.remove(&port.port_name);
                } else if rescan {
                    session.read_name();
                }
            } else if rescan {
                match Session::open(&port.port_name) {
                    Ok(mut s) => {
                        if self
                            .sessions
                            .values()
                            .any(|other| other.channel.uid == s.channel.uid)
                        {
                            failures.push("중복 MCU UID를 발견했습니다.".into());
                            continue;
                        }
                        s.read_name();
                        self.sessions.insert(port.port_name, s);
                    }
                    Err(e) => failures.push(format!("{}: {e}", port.port_name)),
                }
            }
        }
        let mut channels: Vec<_> = self.sessions.values().map(|s| s.channel.clone()).collect();
        match crate::platform::usb_nodes() {
            Ok(nodes) => topology::pair(&nodes, &mut channels),
            Err(e) => failures.push(format!("USB 위치 조회 실패: {e}")),
        }
        channels.sort_by(|a, b| {
            a.board_id
                .cmp(&b.board_id)
                .then(a.usb_port.cmp(&b.usb_port))
        });
        self.revision += 1;
        Inventory {
            revision: self.revision,
            observed_at: now_ms(),
            channels,
            error: failures.join(" · "),
        }
    }
}
