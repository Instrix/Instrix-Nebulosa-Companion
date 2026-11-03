// The UI's handle on the sample buffer, which lives in the Rust core (native
// backend or WASM): every sample at full rate plus a summary pyramid. The UI
// asks for what it draws, when it draws it:
//   lod(t0, t1, columns)  about one row per pixel column, or the samples
//                         themselves when zoomed in that far
//   stats(t0, t1)         exact statistics and integrals over a span
//   valueAt(t)            one sample
// Meta (a few dozen times a second) says what the buffer holds.

import type { Link } from './device';
import { SIGNALS, type Meta, type RawMoments, type RawSpanStats, type Signal } from './nbp';

/** count / mean / M2 / min / max of one channel, with the derived values */
export class Moments {
  n = 0;
  mean = 0;
  m2 = 0;
  min = Infinity;
  max = -Infinity;

  static from(r: RawMoments | undefined): Moments {
    const m = new Moments();
    if (r && r.n) Object.assign(m, { n: r.n, mean: r.mean, m2: r.m2, min: r.min, max: r.max });
    return m;
  }

  /** sample standard deviation */
  get std() {
    return this.n > 1 ? Math.sqrt(this.m2 / (this.n - 1)) : 0;
  }
  get rms() {
    return this.n ? Math.sqrt(this.mean * this.mean + this.m2 / this.n) : 0;
  }
  get pp() {
    return this.n ? this.max - this.min : 0;
  }
}

export type SpanStats = Record<Signal, Moments> & {
  /** first and last sample's time, s */
  t0: number;
  t1: number;
  /** C, J */
  charge: number;
  energy: number;
};

export function spanStats(r: RawSpanStats | null | undefined): SpanStats {
  return {
    i: Moments.from(r?.m.i),
    v: Moments.from(r?.m.v),
    p: Moments.from(r?.m.p),
    m: Moments.from(r?.m.m),
    t0: r?.t0 ?? NaN,
    t1: r?.t1 ?? NaN,
    charge: r?.charge ?? 0,
    energy: r?.energy ?? 0,
  };
}

type Col = (number | null)[];

/** Per row: time, then each channel's min, max, mean, mean ∓ σ; null = gap. */
export interface Lod {
  t: number[];
  /** samples per row */
  n: number[];
  /** each row is one sample (zoomed in past one sample per column) */
  raw: boolean;
  ch: Record<Signal, { min: Col; max: Col; avg: Col; sdLo: Col; sdHi: Col }>;
  /** where samples are missing (stream gaps), s */
  gaps: number[];
}

/** One sample's values per channel, or null (none there). */
export type PointValue = Record<Signal, number> | null;

const LOD_LAYOUT = 3;
const LOD_COLS = 18;

export function emptyLod(): Lod {
  const mk = () => ({ min: [] as Col, max: [] as Col, avg: [] as Col, sdLo: [] as Col, sdHi: [] as Col });
  return { t: [], n: [], raw: false, ch: { i: mk(), v: mk(), p: mk(), m: mk() }, gaps: [] };
}

export function decodeLod(f: Float64Array): Lod {
  const out = emptyLod();
  if (f[0] !== LOD_LAYOUT || f[2] !== LOD_COLS) throw new Error(`lod layout ${f[0]}/${f[2]} not supported`);
  const rows = f[1];
  out.raw = f[3] === 1;
  const col = (k: number) => f.subarray(4 + k * rows, 4 + (k + 1) * rows);
  const t = col(0);
  const n = col(1);
  out.t = Array.from(t);
  out.n = Array.from(n);
  const nul = (x: number) => (Number.isFinite(x) ? x : null);
  SIGNALS.forEach((s, ci) => {
    const [mn, mx, avg, sd] = [col(2 + ci * 4), col(3 + ci * 4), col(4 + ci * 4), col(5 + ci * 4)];
    const c = out.ch[s];
    for (let k = 0; k < rows; k++) {
      c.min.push(nul(mn[k]));
      c.max.push(nul(mx[k]));
      c.avg.push(nul(avg[k]));
      c.sdLo.push(nul(avg[k] - sd[k]));
      c.sdHi.push(nul(avg[k] + sd[k]));
    }
  });
  for (let k = 0; k < rows; k++) if (n[k] === 0) out.gaps.push(t[k]);
  return out;
}

export class SampleStore {
  private link: Link | null = null;
  /** time of the oldest / newest sample held, s */
  tStart = 0;
  tEnd = 0;
  /** changes whenever the buffer does, for redraw scheduling */
  version = 0;
  empty = true;
  /** s per sample */
  samplePeriod = 1 / 2e6;
  bufferSamples = 0;

  attach(link: Link | null) {
    this.link = link;
    this.clear();
  }

  clear() {
    this.tStart = this.tEnd = 0;
    this.empty = true;
    this.version++;
  }

  update(m: Meta) {
    this.empty = !m.has_data;
    this.tStart = m.t_start;
    this.tEnd = m.t_end;
    this.samplePeriod = m.sample_period;
    this.bufferSamples = m.buffer_samples;
    this.version = m.version * 2 + 1; // distinct from clear()'s bumps
  }

  async lod(t0: number, t1: number, bins: number): Promise<Lod> {
    if (!this.link || this.empty || !(t1 > t0)) return emptyLod();
    return decodeLod(await this.link.lod(t0, t1, Math.max(1, Math.round(bins))));
  }

  async stats(t0: number, t1: number): Promise<SpanStats> {
    if (!this.link || this.empty) return spanStats(null);
    return spanStats(await this.link.stats(Math.min(t0, t1), Math.max(t0, t1)));
  }

  async valueAt(t: number): Promise<PointValue> {
    if (!this.link || this.empty) return null;
    const r = await this.link.valueAt(t);
    return r ? { i: r[0], v: r[1], p: r[0] * r[1], m: r[3] ?? NaN } : null;
  }
}
