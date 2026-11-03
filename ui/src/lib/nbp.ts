// NBP on the UI side: constants, and the Rust core (nbp-core, built to WASM)
// for encoding payloads and parsing responses. Layouts live only in Rust.

import init, * as core from './nbp-wasm/nbp_core.js';

export { core };

let ready: Promise<unknown> | null = null;
export function initCore(): Promise<unknown> {
  ready ??= init();
  return ready;
}

export const USB_VID = 0x1a86;
export const USB_PID_HS = 0xfe0d;
export const USB_PID_FS = 0xfe0c;
export const USB_CLASS = 0xff;
export const USB_SUBCLASS = 0x4e;
export const USB_PROTOCOL = 0x01;

export const CMD = {
  PING: 0x01,
  GET_INFO: 0x02,
  REG_READ: 0x03,
  REG_WRITE: 0x04,
  RESET: 0x05,
  STREAM_START: 0x10,
  STREAM_STOP: 0x11,
  GET_STATS: 0x12,
  SET_SOURCE: 0x20,
  SET_RANGE: 0x21,
  GET_RANGE: 0x22,
  SET_DECIMATION: 0x30,
  BOARD_STATUS: 0x38,
} as const;

export const CAP = { USB_HS: 1, RANGE: 2, DECIM: 4, ETH: 8, BOARD: 16, USB_SS: 32 } as const;
export const START_FLUSH = 1;
export const SRC = { STUB_RANDOM: 0, STUB_PATTERN: 1, AD7387: 2 } as const;
export const FPGA_ID_FORMAT5 = 0x4e42000b;

export interface Info {
  proto_version: number;
  fw_version: number;
  fw_version_str: string;
  fpga_id: number;
  base_rate_hz: number;
  link: number;
  link_name: string;
  link_unit: number;
  block_words: number;
  caps: number;
  build: string;
  mac: string;
  ip: string;
}

export interface Decim {
  log2n: number;
  mode: number;
  rate_hz: number;
}

export interface StreamCounters {
  samples: number;
  dirty: number;
  blocks: number;
  empty: number;
  late: number;
  ovf_blocks: number;
  range_changes: number;
  seq_errors: number;
  malformed: number;
  unframed: number;
  gap_periods: number;
}

export interface DataStats {
  blocks: number;
  lost_blocks: number;
  stall_flags: number;
  gap_flags: number;
  test_blocks: number;
  bad_blocks: number;
  bytes: number;
  resyncs: number;
  stream: StreamCounters;
}

/** One channel's point columns (SI units: A, V, W). m2: sum of squared deviations. */
export interface ChanCols {
  min: Float64Array;
  max: Float64Array;
  avg: Float64Array;
  m2: Float64Array;
}

/** Display points (Batch::to_f64 in nbp-core); n = 0 marks a gap point (values NaN). */
export interface Points {
  len: number;
  t: Float64Array;
  n: Float64Array;
  range: Float64Array;
  dirty: Float64Array;
  i: ChanCols;
  v: ChanCols;
  p: ChanCols;
}

export const SIGNALS = ['i', 'v', 'p'] as const;
export type Signal = (typeof SIGNALS)[number];

const BATCH_LAYOUT = 1;
const BATCH_COLS = 16;

export function decodePoints(f: Float64Array): Points {
  if (f[0] !== BATCH_LAYOUT || f[2] !== BATCH_COLS) throw new Error(`point layout ${f[0]}/${f[2]} not supported`);
  const len = f[1];
  let o = 3;
  const col = () => {
    const c = f.subarray(o, o + len);
    o += len;
    return c;
  };
  const chan = (): ChanCols => ({ min: col(), max: col(), avg: col(), m2: col() });
  const t = col(), n = col(), range = col(), dirty = col();
  return { len, t, n, range, dirty, i: chan(), v: chan(), p: chan() };
}

export interface Accum {
  /** C */
  charge: number;
  /** J */
  energy: number;
  /** s of samples integrated */
  seconds: number;
}

export interface Meta {
  stats: DataStats;
  accum: Accum;
  log2n: number;
  stream_id: number;
}

/** units.rs Calibration */
export interface Calibration {
  i_fs: number[];
  i_offset: number[];
  v_scale: number;
  v_offset: number;
}

export interface Reply {
  ty: number;
  status: number;
  payload: Uint8Array;
}

export class NbpError extends Error {
  constructor(
    public cmd: number,
    public status: number,
  ) {
    super(`command 0x${cmd.toString(16).padStart(2, '0')}: ${core.status_name(status)}`);
  }
}

export function hex(v: number, digits = 8): string {
  return '0x' + (v >>> 0).toString(16).toUpperCase().padStart(digits, '0');
}
