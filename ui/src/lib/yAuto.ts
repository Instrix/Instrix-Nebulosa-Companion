/**
 * The y axis in auto: steady. It keeps its range while the data stays inside
 * it (clear of the edges) and fills a fair part of it; only when the data
 * gets close to an edge, goes past one, or shrinks to a sliver does it fit
 * again, with headroom so the next small change doesn't move it either.
 */
export type YRange = [number, number];

/** data nearer an edge than this (of the range) refits */
const EDGE = 0.03;
/** data filling less than this (of the range) refits, tighter */
const FILL = 0.3;
/** headroom on a refit, each side (of the data's span) */
const PAD = 0.15;

function fit(lo: number, hi: number): YRange {
  const pad = Math.max((hi - lo) * PAD, Math.abs(hi) * 1e-3, 1e-12);
  return [lo - pad, hi + pad];
}

/** a linear axis: the range to show for data in [lo, hi], given the one shown */
export function steadyLinear(prev: YRange | null, lo: number, hi: number): YRange {
  if (prev && isFinite(prev[0]) && isFinite(prev[1]) && prev[1] > prev[0]) {
    const s = prev[1] - prev[0];
    const inside = lo >= prev[0] + s * EDGE && hi <= prev[1] - s * EDGE;
    const fills = hi - lo >= s * FILL || s <= fit(lo, hi)[1] - fit(lo, hi)[0] + 1e-15;
    if (inside && fills) return prev;
  }
  return fit(lo, hi);
}

/** a log axis: the same, in decades */
export function steadyLog(prev: YRange | null, lo: number, hi: number): YRange {
  const L = (r: YRange): YRange => [Math.log10(r[0]), Math.log10(r[1])];
  const want: YRange = [lo / 1.5, hi * 1.5];
  if (prev && prev[0] > 0 && prev[1] > prev[0]) {
    const [p0, p1] = L(prev), [l, h] = L([lo, hi]);
    const s = p1 - p0;
    const inside = l >= p0 + s * EDGE && h <= p1 - s * EDGE;
    const fills = h - l >= s * FILL || s <= Math.log10(want[1] / want[0]) + 1e-9;
    if (inside && fills) return prev;
  }
  return want;
}
