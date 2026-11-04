// The side-channel campaign engine behind a link: in the browser builds a
// WASM Campaign (sca.rs) in the core host beside the pipeline it cuts traces
// from (coreLink.ts); the desktop app keeps it in Rust (native.ts). One
// campaign at a time, kept across connections (a new connection can add to
// it at the same rate).

import type { AlignResult, ScaConfig, ScaInfo, ScaRequest, ScaResult, TraceMeta } from './nbp';

export interface ScaEngine {
  /** a new, empty campaign (the old one is dropped) */
  create(cfg: ScaConfig): Promise<void>;
  /** the buffer's [t0, t1) as one more trace (it must hold `samples` of them without a gap): the count */
  add(t0: number, t1: number, meta: TraceMeta): Promise<number>;
  info(): Promise<ScaInfo | null>;
  /** shifts that line every trace's samples [a, b) up, within ±maxShift */
  align(a: number, b: number, maxShift: number): Promise<AlignResult>;
  clearAlign(): Promise<void>;
  query(req: ScaRequest): Promise<ScaResult>;
  /** the campaign as an .npz */
  exportNpz(): Promise<Uint8Array>;
  /** the per-trace metadata (inputs, outputs, groups, widths) */
  meta(): Promise<TraceMeta[]>;
  /** the campaign as a flat .nbsca binary, and loading one back */
  save(): Promise<Uint8Array>;
  load(bytes: Uint8Array): Promise<ScaInfo | null>;
}

