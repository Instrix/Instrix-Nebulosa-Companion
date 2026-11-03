// Point history for the plots, the overview and the meter.
//
// Batches (typed columns, already in A / V / W) are kept as chunks of about
// 40 ms and trimmed by age. Each chunk keeps its own per-channel summary, so
// work over long spans (the overview, statistics over minutes) goes chunk by
// chunk wherever a whole chunk falls inside one column / the span.
//
// Statistics are exact over the raw samples: every point carries its count,
// mean and M2 (sum of squared deviations), merged with Chan et al.'s
// parallel variance.

import { SIGNALS, type ChanCols, type Points, type Signal } from './nbp';

/** count, mean, M2, min, max of one channel */
export class Moments {
  n = 0;
  mean = 0;
  m2 = 0;
  min = Infinity;
  max = -Infinity;

  add(n: number, mean: number, m2: number, min: number, max: number) {
    if (n <= 0) return;
    const tot = this.n + n;
    const d = mean - this.mean;
    this.mean += (d * n) / tot;
    this.m2 += m2 + (d * d * this.n * n) / tot;
    this.n = tot;
    if (min < this.min) this.min = min;
    if (max > this.max) this.max = max;
  }

  merge(o: Moments) {
    this.add(o.n, o.mean, o.m2, o.min, o.max);
  }

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
  /** first and last point's time, s */
  t0: number;
  t1: number;
};

export function emptyStats(): SpanStats {
  return { i: new Moments(), v: new Moments(), p: new Moments(), t0: NaN, t1: NaN };
}

function widen(s: SpanStats, t0: number, t1: number) {
  if (!(t0 >= s.t0)) s.t0 = t0;
  if (!(t1 <= s.t1)) s.t1 = t1;
}

function addPoint(s: SpanStats, b: Points, k: number) {
  const n = b.n[k];
  for (const sig of SIGNALS) {
    const c: ChanCols = b[sig];
    s[sig].add(n, c.avg[k], c.m2[k], c.min[k], c.max[k]);
  }
}

/** Every point of `b` into `s` (the since-start statistics). */
export function addPoints(s: SpanStats, b: Points) {
  for (let k = 0; k < b.len; k++) {
    if (!b.n[k]) continue;
    addPoint(s, b, k);
    widen(s, b.t[k], b.t[k]);
  }
}

class Chunk {
  readonly sum: Record<Signal, Moments> = { i: new Moments(), v: new Moments(), p: new Moments() };
  readonly gaps: boolean;

  constructor(readonly b: Points) {
    let gaps = false;
    for (let k = 0; k < b.len; k++) {
      const n = b.n[k];
      if (!n) {
        gaps = true;
        continue;
      }
      for (const sig of SIGNALS) {
        const c = b[sig];
        this.sum[sig].add(n, c.avg[k], c.m2[k], c.min[k], c.max[k]);
      }
    }
    this.gaps = gaps;
  }

  get t0() {
    return this.b.t[0];
  }
  get t1() {
    return this.b.t[this.b.len - 1];
  }
}

/** Per pixel column: time, then each channel's min, max, mean; null = gap. */
export interface Lod {
  t: number[];
  ch: Record<Signal, { min: (number | null)[]; max: (number | null)[]; avg: (number | null)[] }>;
}

export class PointStore {
  private chunks: Chunk[] = [];
  /** time of the oldest / newest point, s */
  tStart = 0;
  tEnd = 0;
  /** bumped on every change, for redraw scheduling */
  version = 0;

  clear() {
    this.chunks = [];
    this.tStart = this.tEnd = 0;
    this.version++;
  }

  get empty() {
    return this.chunks.length === 0;
  }

  /** Adds points, drops chunks older than `keepS`. */
  append(b: Points, keepS: number) {
    if (!b.len) return;
    const c = new Chunk(b);
    this.chunks.push(c);
    this.tEnd = c.t1;
    const cut = this.tEnd - keepS;
    while (this.chunks.length > 1 && this.chunks[0].t1 < cut) this.chunks.shift();
    this.tStart = this.chunks[0].t0;
    this.version++;
  }

  stats(t0: number, t1: number): SpanStats {
    const s = emptyStats();
    for (const c of this.chunks) {
      if (c.t1 < t0 || c.t0 > t1) continue;
      if (c.t0 >= t0 && c.t1 <= t1) {
        if (c.sum.i.n) {
          for (const sig of SIGNALS) s[sig].merge(c.sum[sig]);
          widen(s, c.t0, c.t1);
        }
        continue;
      }
      const b = c.b;
      for (let k = 0; k < b.len; k++) {
        const t = b.t[k];
        if (!b.n[k] || t < t0 || t > t1) continue;
        addPoint(s, b, k);
        widen(s, t, t);
      }
    }
    return s;
  }

  /** [t0, t1] in `bins` columns: per column the extremes and the count-weighted means. */
  lod(t0: number, t1: number, bins: number): Lod {
    const mk = () => ({ min: [] as (number | null)[], max: [] as (number | null)[], avg: [] as (number | null)[] });
    const out: Lod = { t: [], ch: { i: mk(), v: mk(), p: mk() } };
    if (!(t1 > t0)) return out;
    const scale = bins / (t1 - t0);
    let bin = -1;
    let bt = 0;
    let n = 0;
    const lo = { i: Infinity, v: Infinity, p: Infinity };
    const hi = { i: -Infinity, v: -Infinity, p: -Infinity };
    const sum = { i: 0, v: 0, p: 0 };
    const flush = () => {
      if (n > 0) {
        out.t.push(bt);
        for (const s of SIGNALS) {
          out.ch[s].min.push(lo[s]);
          out.ch[s].max.push(hi[s]);
          out.ch[s].avg.push(sum[s] / n);
          lo[s] = Infinity;
          hi[s] = -Infinity;
          sum[s] = 0;
        }
      }
      n = 0;
    };
    const gap = (t: number) => {
      flush();
      out.t.push(t);
      for (const s of SIGNALS) {
        out.ch[s].min.push(null);
        out.ch[s].max.push(null);
        out.ch[s].avg.push(null);
      }
      bin = -1;
    };
    const column = (tk: number) => {
      const b = Math.floor((tk - t0) * scale);
      if (b !== bin) {
        flush();
        bin = b;
        bt = tk;
      }
    };
    for (const c of this.chunks) {
      if (c.t1 < t0 || c.t0 > t1) continue;
      // a whole chunk inside one column: its summary will do
      if (!c.gaps && c.t0 >= t0 && c.t1 <= t1 && Math.floor((c.t0 - t0) * scale) === Math.floor((c.t1 - t0) * scale)) {
        column(c.t0);
        const nk = c.sum.i.n;
        n += nk;
        for (const s of SIGNALS) {
          const m = c.sum[s];
          if (m.min < lo[s]) lo[s] = m.min;
          if (m.max > hi[s]) hi[s] = m.max;
          sum[s] += m.mean * nk;
        }
        continue;
      }
      const b = c.b;
      for (let k = 0; k < b.len; k++) {
        const tk = b.t[k];
        if (tk < t0 || tk > t1) continue;
        const nk = b.n[k];
        if (!nk) {
          gap(tk);
          continue;
        }
        column(tk);
        n += nk;
        for (const s of SIGNALS) {
          const cc = b[s];
          if (cc.min[k] < lo[s]) lo[s] = cc.min[k];
          if (cc.max[k] > hi[s]) hi[s] = cc.max[k];
          sum[s] += cc.avg[k] * nk;
        }
      }
    }
    flush();
    return out;
  }
}
