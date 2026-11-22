use std::io;

#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub kind: u8,
    pub channel: u8,
    pub opcode: u16,
    pub session: u64,
    pub sequence: u32,
    pub payload: Vec<u8>,
}
impl Frame {
    pub fn encode(&self) -> io::Result<Vec<u8>> {
        if self.payload.len() > 1024 || self.channel > 5 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "frame bounds"));
        }
        let mut b = Vec::with_capacity(28 + self.payload.len());
        b.extend_from_slice(&0x4d4bu16.to_le_bytes());
        b.extend_from_slice(&[1, self.kind, 0, self.channel]);
        b.extend_from_slice(&self.opcode.to_le_bytes());
        b.extend_from_slice(&self.session.to_le_bytes());
        b.extend_from_slice(&self.sequence.to_le_bytes());
        b.extend_from_slice(&(self.payload.len() as u16).to_le_bytes());
        b.extend_from_slice(&[0, 0]);
        b.extend_from_slice(&self.payload);
        b.extend_from_slice(&crc32fast::hash(&b).to_le_bytes());
        let mut out = cobs::encode_vec(&b);
        out.push(0);
        if out.len().is_multiple_of(64) {
            out.push(0);
        }
        Ok(out)
    }
    pub fn decode(encoded: &[u8]) -> io::Result<Self> {
        let bad = || io::Error::new(io::ErrorKind::InvalidData, "invalid management frame");
        let b = cobs::decode_vec(encoded).map_err(|_| bad())?;
        if b.len() < 28
            || b[0..3] != [0x4b, 0x4d, 1]
            || b[4] != 0
            || b[5] > 5
            || b[22..24] != [0, 0]
        {
            return Err(bad());
        }
        let n = u16::from_le_bytes([b[20], b[21]]) as usize;
        if n > 1024
            || b.len() != 28 + n
            || crc32fast::hash(&b[..24 + n])
                != u32::from_le_bytes(b[24 + n..].try_into().map_err(|_| bad())?)
        {
            return Err(bad());
        }
        Ok(Self {
            kind: b[3],
            channel: b[5],
            opcode: u16::from_le_bytes([b[6], b[7]]),
            session: u64::from_le_bytes(b[8..16].try_into().map_err(|_| bad())?),
            sequence: u32::from_le_bytes(b[16..20].try_into().map_err(|_| bad())?),
            payload: b[24..24 + n].to_vec(),
        })
    }
}
#[derive(Default)]
pub struct Decoder {
    encoded: Vec<u8>,
    discard: bool,
    pub rejected: u64,
}
impl Decoder {
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<Frame> {
        let mut frames = Vec::new();
        for &b in bytes {
            if b == 0 {
                if !self.discard && !self.encoded.is_empty() {
                    match Frame::decode(&self.encoded) {
                        Ok(f) => frames.push(f),
                        Err(_) => self.rejected += 1,
                    }
                }
                self.encoded.clear();
                self.discard = false;
            } else if !self.discard {
                if self.encoded.len() >= 1057 {
                    self.encoded.clear();
                    self.discard = true;
                    self.rejected += 1;
                } else {
                    self.encoded.push(b);
                }
            }
        }
        frames
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matches_actual_electron_v1_wire_vectors() {
        let vectors: serde_json::Value =
            serde_json::from_str(include_str!("../tests/v1-wire-vectors.json")).unwrap();
        let hex = |s: &str| {
            s.as_bytes()
                .chunks_exact(2)
                .map(|b| u8::from_str_radix(std::str::from_utf8(b).unwrap(), 16).unwrap())
                .collect::<Vec<_>>()
        };
        for v in vectors.as_array().unwrap() {
            let f = Frame {
                kind: v["kind"].as_u64().unwrap() as u8,
                channel: v["channel"].as_u64().unwrap() as u8,
                opcode: v["opcode"].as_u64().unwrap() as u16,
                session: v["session"].as_str().unwrap().parse().unwrap(),
                sequence: v["sequence"].as_u64().unwrap() as u32,
                payload: hex(v["payload"].as_str().unwrap()),
            };
            let expected = hex(v["wire"].as_str().unwrap());
            assert_eq!(f.encode().unwrap(), expected);
            assert_eq!(Decoder::default().feed(&expected), vec![f]);
        }
    }
    #[test]
    fn every_boundary_crc_and_corrupt_stream_recovery() {
        let f = Frame {
            kind: 1,
            channel: 3,
            opcode: 0x301,
            session: 0xfedcba9876543210,
            sequence: 123,
            payload: (0..=255).cycle().take(1024).collect(),
        };
        let wire = f.encode().unwrap();
        for split in 0..wire.len() {
            let mut d = Decoder::default();
            let mut got = d.feed(&wire[..split]);
            got.extend(d.feed(&wire[split..]));
            assert_eq!(got, vec![f.clone()]);
        }
        let mut d = Decoder::default();
        let mut bad = wire.clone();
        bad[10] ^= 0x40;
        assert!(d.feed(&bad).is_empty());
        d.feed(&vec![1; 2000]);
        d.feed(&[0]);
        assert_eq!(d.feed(&wire), vec![f]);
        assert!(d.rejected >= 2);
    }
}
