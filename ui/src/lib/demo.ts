// Synthetic data for UI work without hardware: open the UI with `?demo`.
// A 3.3 V rail feeding a load that idles at ~200 µA and wakes every 500 ms
// for a 40 ms burst of ~25 mA, with noise. `?demo&sel=a,b` also selects
// [a, b] s before the newest point (for screenshots).

import type { Meta, Points } from './nbp';

const POINTS_PER_S = 4000;
const TICK_MS = 40;
const SAMPLES_PER_POINT = 500; // 2 MS/s

export function startDemo(onPoints: (p: Points) => void, onMeta: (m: Meta) => void): () => void {
  let t = 0;
  let charge = 0;
  let energy = 0;
  let id = 0;
  const per = (POINTS_PER_S * TICK_MS) / 1000;
  const tick = () => {
    const len = per;
    const f = new Float64Array(len * 16);
    const col = (k: number) => f.subarray(k * len, (k + 1) * len);
    const [T, N, R, D] = [col(0), col(1), col(2), col(3)];
    const ch = (o: number) => ({ min: col(o), max: col(o + 1), avg: col(o + 2), m2: col(o + 3) });
    const I = ch(4), V = ch(8), P = ch(12);
    for (let k = 0; k < len; k++) {
      const phase = t % 0.5;
      const burst = phase < 0.04;
      const base = burst ? 25e-3 + 3e-3 * Math.sin(phase * 900) : 200e-6 + 20e-6 * Math.sin(t * 7);
      const noise = burst ? 2e-3 : 30e-6;
      const i = base + (Math.random() - 0.5) * noise * 0.3;
      const v = 3.3 - i * 0.8 + (Math.random() - 0.5) * 2e-3;
      T[k] = t;
      N[k] = SAMPLES_PER_POINT;
      R[k] = burst ? 2 : 4;
      D[k] = 0;
      const set = (c: typeof I, mean: number, spread: number) => {
        c.avg[k] = mean;
        c.min[k] = mean - spread * (0.5 + Math.random() * 0.5);
        c.max[k] = mean + spread * (0.5 + Math.random() * 0.5);
        c.m2[k] = ((spread / 3) ** 2) * SAMPLES_PER_POINT;
      };
      set(I, i, noise);
      set(V, v, 1e-3);
      set(P, i * v, noise * v);
      charge += i / POINTS_PER_S;
      energy += (i * v) / POINTS_PER_S;
      t += 1 / POINTS_PER_S;
    }
    onPoints({ len, t: T, n: N, range: R, dirty: D, i: I, v: V, p: P });
    onMeta({
      stats: {
        blocks: id, lost_blocks: 0, stall_flags: 0, gap_flags: 0, test_blocks: 0, bad_blocks: 0,
        bytes: Math.round(t * 8e6), resyncs: 0,
        stream: { samples: Math.round(t * 2e6), dirty: 0, blocks: 0, empty: 0, late: 0, ovf_blocks: 0, range_changes: 0, seq_errors: 0, malformed: 0, unframed: 0, gap_periods: 0 },
      },
      accum: { charge, energy, seconds: t },
      log2n: 0,
      stream_id: 1,
    });
    id++;
  };
  const timer = setInterval(tick, TICK_MS);
  return () => clearInterval(timer);
}

export const demoRequested = typeof location !== 'undefined' && new URLSearchParams(location.search).has('demo');

/** `sel=a,b`: seconds before the newest point */
export function demoSelection(): [number, number] | null {
  const s = new URLSearchParams(location.search).get('sel');
  if (!s) return null;
  const [a, b] = s.split(',').map(Number);
  return isFinite(a) && isFinite(b) ? [a, b] : null;
}
