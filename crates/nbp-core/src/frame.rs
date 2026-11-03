//! Message framing: the 8-byte header and a parser that cuts a byte stream
//! into messages, resyncing on the magic when it finds garbage.

use crate::le16;

pub const MAGIC: u16 = 0x424E; // bytes 'N' 'B'
pub const HDR_LEN: usize = 8;
pub const MAX_PAYLOAD: usize = 504; // commands and responses
pub const BLOCK_BYTES: usize = 2048; // DATA messages, header included
const MAX_LEN: usize = BLOCK_BYTES - HDR_LEN;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Frame {
    pub ty: u8,
    pub status: u8,
    pub seq: u16,
    pub payload: Vec<u8>,
}

/// One message: header + payload.
pub fn encode(ty: u8, seq: u16, payload: &[u8]) -> Vec<u8> {
    assert!(payload.len() <= MAX_PAYLOAD, "NBP payload too long");
    let mut v = Vec::with_capacity(HDR_LEN + payload.len());
    v.extend_from_slice(&MAGIC.to_le_bytes());
    v.push(ty);
    v.push(0);
    v.extend_from_slice(&seq.to_le_bytes());
    v.extend_from_slice(&(payload.len() as u16).to_le_bytes());
    v.extend_from_slice(payload);
    v
}

/// Cuts one direction's byte stream into messages. Parse by `len`; transfer
/// boundaries mean nothing.
#[derive(Default)]
pub struct FrameParser {
    buf: Vec<u8>,
    pos: usize,
    /// Times bytes were discarded to find the next magic.
    pub resyncs: u64,
}

impl FrameParser {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, data: &[u8]) {
        if self.pos > 0 && self.pos * 2 >= self.buf.len() {
            self.buf.drain(..self.pos);
            self.pos = 0;
        }
        self.buf.extend_from_slice(data);
    }

    pub fn clear(&mut self) {
        self.buf.clear();
        self.pos = 0;
    }

    pub fn next_frame(&mut self) -> Option<Frame> {
        loop {
            let b = &self.buf[self.pos..];
            if b.len() < 2 {
                return None;
            }
            if b[0] != b'N' || b[1] != b'B' {
                self.resyncs += 1;
                // skip to the next "NB", or keep a trailing 'N'
                match b[1..].windows(2).position(|w| w == b"NB") {
                    Some(i) => self.pos += 1 + i,
                    None => self.pos += if b[b.len() - 1] == b'N' { b.len() - 1 } else { b.len() },
                }
                continue;
            }
            if b.len() < HDR_LEN {
                return None;
            }
            let len = le16(b, 6) as usize;
            if len > MAX_LEN {
                self.resyncs += 1;
                self.pos += 1;
                continue;
            }
            if b.len() < HDR_LEN + len {
                return None;
            }
            let f = Frame {
                ty: b[2],
                status: b[3],
                seq: le16(b, 4),
                payload: b[HDR_LEN..HDR_LEN + len].to_vec(),
            };
            self.pos += HDR_LEN + len;
            return Some(f);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_split_and_garbage() {
        let a = encode(0x41, 7, b"hello");
        let b = encode(0x42, 8, &[]);
        let mut stream = vec![0xFF, b'N', 0x00];
        stream.extend_from_slice(&a);
        stream.extend_from_slice(&b);
        let mut p = FrameParser::new();
        let mut out = vec![];
        for chunk in stream.chunks(3) {
            p.push(chunk);
            while let Some(f) = p.next_frame() {
                out.push(f);
            }
        }
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].seq, 7);
        assert_eq!(out[0].payload, b"hello");
        assert_eq!(out[1].ty, 0x42);
        assert!(p.resyncs > 0);
    }
}
