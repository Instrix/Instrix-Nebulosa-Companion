// A plot's visible span gliding to where the view wants it: zooms and pans
// animate instead of jumping. Width moves geometrically (a 2× zoom looks the
// same at any scale) and the point that stays put on screen (under the
// cursor, for a zoom) stays put all the way.

export type Span = [number, number];

export class Glide {
  /** the span on screen now */
  shown: Span | null = null;
  private last = 0;

  /** `tau`: the time constant, ms (≈ 3·tau to arrive) */
  constructor(private tau = 50) {}

  /** jump there (no overlap with what's shown: a glide would only sweep across) */
  snap(target: Span) {
    this.shown = [target[0], target[1]];
  }

  /**
   * One frame toward `target`. `snapPan`: a pure pan jumps (live data
   * scrolling must not lag). Returns true while still moving.
   */
  step(now: number, target: Span, snapPan = false): boolean {
    const dt = this.last ? Math.min(64, now - this.last) : 16;
    this.last = now;
    const s = this.shown;
    const [ta, tb] = target;
    const tw = tb - ta;
    if (!s || !(tw > 0)) {
      this.snap(target);
      return false;
    }
    const [a, b] = s;
    const w = b - a;
    const sameWidth = Math.abs(w - tw) <= tw * 1e-3;
    // already there, a pan that should jump, or nothing in common: no glide
    if ((sameWidth && (snapPan || Math.abs(a - ta) <= tw * 1e-3)) || b < ta || a > tb || !(w > 0)) {
      this.snap(target);
      return false;
    }
    const k = 1 - Math.exp(-dt / this.tau);
    if (sameWidth) {
      const na = a + (ta - a) * k;
      this.shown = [na, na + tw];
      return true;
    }
    // the fixed point p: at the same place on screen in both spans
    const p = (ta * w - a * tw) / (w - tw);
    const f = (p - a) / w;
    const nw = Math.exp(Math.log(w) + (Math.log(tw) - Math.log(w)) * k);
    // a fixed point far outside (zoom and pan at once): just move the centre
    if (!isFinite(p) || f < -1 || f > 2) {
      const c = (a + b) / 2 + ((ta + tb) / 2 - (a + b) / 2) * k;
      this.shown = [c - nw / 2, c + nw / 2];
    } else this.shown = [p - f * nw, p + (1 - f) * nw];
    if (Math.abs(nw - tw) <= tw * 5e-3) {
      this.snap(target);
      return false;
    }
    return true;
  }
}

/** both spans */
export function union(a: Span, b: Span): Span {
  return [Math.min(a[0], b[0]), Math.max(a[1], b[1])];
}
