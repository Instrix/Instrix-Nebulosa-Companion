//! FPGA sample stream, format 5 (FPGA ID 0x4E42000B on). A port of the
//! firmware's `common/fpga/neb_stream.c`; keep the two in step.
//!
//! Every FIFO word (host order, after byte-swapping) is one of, by bits 31:29:
//!   sample  1 d 0   [30] DIRTY  [27:14] current  [13:0] voltage (14-bit codes)
//!   header  0 1 0   [28:27] type, then:
//!     BLOCK (0)  [26:11] block seq  [10] OVF  [9] decim mode  [8:5] log2 N
//!                [4:2] range  [1] source (0 stub, 1 ADC)
//!     TS_HI (1)  [23:0] timestamp[47:24]
//!     TS_LO (2)  [23:0] timestamp[23:0]
//!   empty   0x00000000           late   0x20000000
//! A block is BLOCK, TS_HI, TS_LO (empty words may fall between), then
//! 1..256 samples; it ends where the next BLOCK begins. Sample k of a block
//! was taken at timestamp + k * 2^log2N, in 500 ns ADC periods.

use serde::Serialize;

pub const LATE_WORD: u32 = 0x2000_0000;
/// ADC sample period: timestamps count these.
pub const TS_PERIOD_S: f64 = 500e-9;

const HDR_BLOCK: u32 = 0;
const HDR_TS_HI: u32 = 1;
const HDR_TS_LO: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sample {
    /// ADC sample periods (500 ns) since FPGA reset
    pub ts: u64,
    /// 2^log2N: the timestamp distance to the next sample in the block
    pub step: u32,
    /// 14-bit code, from the chain of `range`
    pub current: u16,
    /// 14-bit code
    pub voltage: u16,
    pub range: u8,
    pub dirty: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Word {
    /// empty, LATE, or a header word (state updated)
    Skip,
    Sample(Sample),
    /// a block header just completed
    Block,
    /// malformed: a word out of place (counted)
    Error,
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct StreamCounters {
    pub samples: u64,
    pub dirty: u64,
    pub blocks: u64,
    pub empty: u64,
    pub late: u64,
    pub ovf_blocks: u64,
    pub range_changes: u64,
    pub seq_errors: u64,
    pub malformed: u64,
    pub unframed: u64,
    pub gap_periods: u64,
}

#[derive(Default)]
pub struct StreamDecoder {
    ts0: u64,
    ts_next: u64,
    step: u32,
    seq: u16,
    k: u16,
    pub range: u8,
    pub dmode: u8,
    pub log2n: u8,
    pub src: u8,
    ovf: bool,
    hdr_need: u8,
    synced: bool,
    have_prev: bool,
    prev_seq: u16,
    prev_ts_next: u64,
    pub c: StreamCounters,
}

impl StreamDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Words were lost upstream: drop samples until the next BLOCK header and
    /// don't count that header's sequence jump again.
    pub fn desync(&mut self) {
        self.synced = false;
        self.have_prev = false;
    }

    pub fn feed(&mut self, w: u32) -> Word {
        if w >> 31 != 0 {
            if !self.synced {
                self.c.unframed += 1;
                return Word::Skip;
            }
            if self.hdr_need != 0 {
                self.c.malformed += 1;
                return Word::Error;
            }
            let s = Sample {
                ts: self.ts_next,
                step: self.step,
                current: ((w >> 14) & 0x3FFF) as u16,
                voltage: (w & 0x3FFF) as u16,
                range: self.range,
                dirty: (w >> 30) & 1 != 0,
            };
            self.ts_next += self.step as u64;
            self.k = self.k.wrapping_add(1);
            self.c.samples += 1;
            self.c.dirty += s.dirty as u64;
            return Word::Sample(s);
        }

        if w >> 29 == 2 {
            match (w >> 27) & 3 {
                HDR_BLOCK => {
                    if self.synced && self.hdr_need != 0 {
                        self.c.malformed += 1; // the last header never completed
                    }
                    if self.synced {
                        self.have_prev = true;
                        self.prev_seq = self.seq;
                        self.prev_ts_next = self.ts_next;
                    }
                    let seq = ((w >> 11) & 0xFFFF) as u16;
                    let range = ((w >> 2) & 7) as u8;
                    if self.have_prev && seq != self.prev_seq.wrapping_add(1) {
                        self.c.seq_errors += 1;
                    }
                    if self.synced && range != self.range {
                        self.c.range_changes += 1;
                    }
                    self.seq = seq;
                    self.ovf = (w >> 10) & 1 != 0;
                    self.dmode = ((w >> 9) & 1) as u8;
                    self.log2n = ((w >> 5) & 0xF) as u8;
                    self.range = range;
                    self.src = ((w >> 1) & 1) as u8;
                    self.step = 1 << self.log2n;
                    self.k = 0;
                    self.hdr_need = 2;
                    self.synced = true;
                    self.c.ovf_blocks += self.ovf as u64;
                    return Word::Skip;
                }
                HDR_TS_HI => {
                    if !self.synced {
                        return Word::Skip;
                    }
                    if self.hdr_need != 2 {
                        self.c.malformed += 1;
                        return Word::Error;
                    }
                    self.ts0 = ((w & 0xFF_FFFF) as u64) << 24;
                    self.hdr_need = 1;
                    return Word::Skip;
                }
                HDR_TS_LO => {
                    if !self.synced {
                        return Word::Skip;
                    }
                    if self.hdr_need != 1 {
                        self.c.malformed += 1;
                        return Word::Error;
                    }
                    self.ts0 |= (w & 0xFF_FFFF) as u64;
                    self.ts_next = self.ts0;
                    self.hdr_need = 0;
                    self.c.blocks += 1;
                    if self.have_prev {
                        if self.ts0 >= self.prev_ts_next {
                            self.c.gap_periods += self.ts0 - self.prev_ts_next;
                        } else {
                            self.c.malformed += 1; // overlaps the previous block
                        }
                    }
                    return Word::Block;
                }
                _ => {
                    self.c.malformed += 1;
                    return Word::Error;
                }
            }
        }

        match w {
            0 => self.c.empty += 1,
            LATE_WORD => self.c.late += 1,
            _ => {
                self.c.malformed += 1;
                return Word::Error;
            }
        }
        Word::Skip
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn block_words(seq: u16, range: u8, log2n: u8, ts: u64, samples: &[(u16, u16, bool)]) -> Vec<u32> {
        let mut v = vec![
            (2 << 29) | ((seq as u32) << 11) | ((log2n as u32) << 5) | ((range as u32) << 2) | 2,
            (2 << 29) | (1 << 27) | ((ts >> 24) as u32 & 0xFF_FFFF),
            0, // empty words may fall between header words
            (2 << 29) | (2 << 27) | (ts as u32 & 0xFF_FFFF),
        ];
        for &(i, u, d) in samples {
            v.push((1 << 31) | ((d as u32) << 30) | ((i as u32) << 14) | u as u32);
        }
        v
    }

    #[test]
    fn decodes_blocks() {
        let mut d = StreamDecoder::new();
        let mut w = block_words(10, 3, 1, 0x1234_5678_9A, &[(100, 200, false), (101, 201, true)]);
        w.extend(block_words(11, 4, 1, 0x1234_5678_9A + 4, &[(5, 6, false)]));
        let s: Vec<Sample> = w
            .iter()
            .filter_map(|&w| match d.feed(w) {
                Word::Sample(s) => Some(s),
                _ => None,
            })
            .collect();
        assert_eq!(s.len(), 3);
        assert_eq!(s[0].ts, 0x1234_5678_9A);
        assert_eq!(s[1].ts, 0x1234_5678_9A + 2);
        assert!(s[1].dirty);
        assert_eq!((s[2].range, s[2].current, s[2].voltage), (4, 5, 6));
        assert_eq!(d.c.seq_errors, 0);
        assert_eq!(d.c.range_changes, 1);
        assert_eq!(d.c.gap_periods, 0);
        assert_eq!(d.c.malformed, 0);
    }
}
