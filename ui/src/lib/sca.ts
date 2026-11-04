// The side-channel campaign engine behind a link: in the browser builds a
// WASM Campaign (sca.rs) cutting traces out of the link's pipeline; the
// desktop app keeps it in Rust (native.ts). One campaign at a time, kept
// across connections (a new connection can add to it at the same rate).

import { core, type AlignResult, type ScaConfig, type ScaInfo, type ScaRequest, type ScaResult, type TraceMeta } from './nbp';

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
}

let campaign: core.Campaign | null = null;

function need(): core.Campaign {
  if (!campaign) throw new Error('no campaign');
  return campaign;
}

/** the engine for a WASM pipeline (WebUSB, demo) */
export function wasmEngine(pipeline: () => core.Pipeline): ScaEngine {
  return {
    async create(cfg) {
      campaign?.free();
      campaign = new core.Campaign(cfg);
    },
    async add(t0, t1, meta) {
      return need().add(pipeline(), t0, t1, meta);
    },
    async info() {
      return campaign ? (campaign.info() as ScaInfo) : null;
    },
    async align(a, b, maxShift) {
      return need().align(a, b, maxShift) as AlignResult;
    },
    async clearAlign() {
      campaign?.clear_align();
    },
    async query(req) {
      return need().query(req) as ScaResult;
    },
    async exportNpz() {
      return need().export_npz();
    },
    async meta() {
      return campaign ? (campaign.meta() as TraceMeta[]) : [];
    },
  };
}
