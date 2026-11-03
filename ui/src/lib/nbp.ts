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

/** current, voltage, power, and the math channel (when there is one) */
export const SIGNALS = ['i', 'v', 'p', 'm'] as const;
export type Signal = (typeof SIGNALS)[number];

/** buffer.rs Moments: one channel over a span (m2: sum of squared deviations) */
export interface RawMoments {
  n: number;
  mean: number;
  m2: number;
  min: number;
  max: number;
}

/** buffer.rs SpanStats */
export interface RawSpanStats {
  m: Record<Signal, RawMoments>;
  t0: number | null;
  t1: number | null;
  /** C */
  charge: number;
  /** J */
  energy: number;
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
  /** every sample since the stream started */
  run: Record<Signal, RawMoments>;
  log2n: number;
  stream_id: number;
  /** what the sample buffer holds */
  has_data: boolean;
  t_start: number;
  t_end: number;
  version: number;
  buffer_samples: number;
  /** s per sample at the current decimation */
  sample_period: number;
  trigger: { count: number; recent: number[] };
  recording: { active: boolean; samples: number; bytes: number; seconds: number };
}

/** trigger.rs Kind */
export type TriggerKind =
  | { type: 'edge'; slope: 'rise' | 'fall' | 'both'; level: number }
  | { type: 'window'; low: number; high: number; when: 'enter' | 'exit' | 'both' }
  | { type: 'pulse'; polarity: 'high' | 'low'; level: number; cond: 'any' | 'gt' | 'lt' | 'within'; min: number; max: number }
  | { type: 'timeout'; polarity: 'high' | 'low'; level: number; time: number };

/** trigger.rs TriggerCfg */
export interface TriggerCfg {
  enabled: boolean;
  source: 'i' | 'v' | 'p' | 'm';
  kind: TriggerKind;
  hysteresis: number;
  holdoff: number;
  /** moving average, s (0: none) */
  filter: number;
}

/** trigger.rs Event: width / peak / mean / integral are NaN (null) for edges */
export interface TrigEvent {
  seq: number;
  t: number;
  /** e rise, f fall, n enter, x exit, p pulse, t timeout */
  what: 'e' | 'f' | 'n' | 'x' | 'p' | 't';
  width: number | null;
  peak: number | null;
  mean: number | null;
  integral: number | null;
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

/** analysis.rs Request */
export type AnalysisRequest =
  | { kind: 'histogram'; source: Signal; bins: number; log: boolean }
  | { kind: 'psd'; source: Signal; nfft: number }
  | { kind: 'maxwindow'; source: Signal; width: number };

/** analysis.rs Result */
export type AnalysisResult =
  | {
      kind: 'histogram';
      edges: number[];
      counts: number[];
      n: number;
      excluded: number;
      mean: number;
      std: number;
      min: number;
      max: number;
      percentiles: [number, number][];
    }
  | { kind: 'psd'; freq: number[]; psd: number[]; segments: number; resolution: number; rms: number }
  | { kind: 'maxwindow'; t: number; width: number; mean: number; span_mean: number }
  | { kind: 'empty'; reason: string };
