//! Strict unfragmented IPv4 TCP/UDP views and checksum-correct endpoint rewriting.
use super::dhcp::checksum;
use std::net::Ipv4Addr;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Key {
    pub protocol: u8,
    pub client: Ipv4Addr,
    pub client_port: u16,
    pub remote: Ipv4Addr,
    pub remote_port: u16,
}
pub struct Packet<'a> {
    pub frame: &'a [u8],
    pub offset: usize,
    pub key: Key,
}
fn u16be(b: &[u8], n: usize) -> u16 {
    u16::from_be_bytes([b[n], b[n + 1]])
}
pub fn packet(frame: &[u8]) -> Option<Packet<'_>> {
    if frame.len() < 42 || frame[12..14] != [8, 0] || frame[14] >> 4 != 4 {
        return None;
    }
    let header = usize::from(frame[14] & 15) * 4;
    let length = usize::from(u16be(frame, 16));
    let protocol = frame[23];
    let minimum = match protocol {
        6 => 20,
        17 => 8,
        _ => return None,
    };
    if header < 20
        || length < header + minimum
        || length + 14 > frame.len()
        || u16be(frame, 20) & 0x3fff != 0
        || checksum(&frame[14..14 + header]) != 0
    {
        return None;
    }
    let offset = 14 + header;
    if protocol == 6
        && (frame[offset + 12] >> 4 < 5
            || usize::from(frame[offset + 12] >> 4) * 4 > length - header)
        || protocol == 17 && usize::from(u16be(frame, offset + 4)) != length - header
    {
        return None;
    }
    Some(Packet {
        frame: &frame[..14 + length],
        offset,
        key: Key {
            protocol,
            client: Ipv4Addr::new(frame[26], frame[27], frame[28], frame[29]),
            remote: Ipv4Addr::new(frame[30], frame[31], frame[32], frame[33]),
            client_port: u16be(frame, offset),
            remote_port: u16be(frame, offset + 2),
        },
    })
}
fn transport_checksum(frame: &[u8], offset: usize) -> u16 {
    let mut b = Vec::with_capacity(12 + frame.len() - offset);
    b.extend_from_slice(&frame[26..34]);
    b.extend_from_slice(&[0, frame[23]]);
    b.extend_from_slice(&((frame.len() - offset) as u16).to_be_bytes());
    b.extend_from_slice(&frame[offset..]);
    checksum(&b)
}
impl Packet<'_> {
    pub fn valid(&self) -> bool {
        self.key.protocol == 17 && self.frame[self.offset + 6..self.offset + 8] == [0, 0]
            || transport_checksum(self.frame, self.offset) == 0
    }
    pub fn syn(&self) -> bool {
        self.key.protocol == 6 && self.frame[self.offset + 13] & 0x17 == 2
    }
    pub fn rewrite(
        &self,
        source: Ipv4Addr,
        source_port: u16,
        dest: Ipv4Addr,
        dest_port: u16,
    ) -> Vec<u8> {
        let mut b = self.frame.to_vec();
        b[26..30].copy_from_slice(&source.octets());
        b[30..34].copy_from_slice(&dest.octets());
        b[self.offset..self.offset + 2].copy_from_slice(&source_port.to_be_bytes());
        b[self.offset + 2..self.offset + 4].copy_from_slice(&dest_port.to_be_bytes());
        b[24..26].fill(0);
        let ip = checksum(&b[14..self.offset]);
        b[24..26].copy_from_slice(&ip.to_be_bytes());
        let at = self.offset + if self.key.protocol == 6 { 16 } else { 6 };
        b[at..at + 2].fill(0);
        let c = transport_checksum(&b, self.offset);
        b[at..at + 2].copy_from_slice(&if c == 0 { u16::MAX } else { c }.to_be_bytes());
        b
    }
}

pub fn tcp_frame(
    mac: [u8; 6],
    dest_mac: [u8; 6],
    key: Key,
    sequence: u32,
    ack: u32,
    flags: u8,
) -> Vec<u8> {
    let mut b = vec![0; 54];
    b[..6].copy_from_slice(&dest_mac);
    b[6..12].copy_from_slice(&mac);
    b[12..14].copy_from_slice(&[8, 0]);
    b[14] = 0x45;
    b[16..18].copy_from_slice(&40u16.to_be_bytes());
    b[22] = 64;
    b[23] = 6;
    b[26..30].copy_from_slice(&key.client.octets());
    b[30..34].copy_from_slice(&key.remote.octets());
    b[38..42].copy_from_slice(&sequence.to_be_bytes());
    b[42..46].copy_from_slice(&ack.to_be_bytes());
    b[46] = 0x50;
    b[47] = flags;
    b[48..50].copy_from_slice(&u16::MAX.to_be_bytes());
    let c = checksum(&b[14..34]);
    b[24..26].copy_from_slice(&c.to_be_bytes());
    packet(&b).expect("fixed IPv4 TCP header").rewrite(
        key.client,
        key.client_port,
        key.remote,
        key.remote_port,
    )
}
