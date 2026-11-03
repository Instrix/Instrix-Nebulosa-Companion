// What the scope screen shows: the time window (LIVE follows the newest
// point; HOLD stays where it was put) and the selection. Shared by the main
// plot, the overview and the statistics.

import type { PointStore } from './store';

/** time/div steps, s (1-2-5) */
export const TIME_DIVS = [0.005, 0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1, 2, 5, 10, 20, 30, 60];
export const DIVS_X = 10;
export const DIVS_Y = 8;

export function fmtTime(s: number, digits = 3): string {
  const a = Math.abs(s);
  if (a === 0) return '0';
  if (a < 1e-3) return `${+(s * 1e6).toPrecision(digits)} µs`;
  if (a < 1) return `${+(s * 1e3).toPrecision(digits)} ms`;
  if (a < 60) return `${+s.toPrecision(digits)} s`;
  const m = Math.floor(a / 60);
  return `${s < 0 ? '-' : ''}${m}:${String(Math.round(a - m * 60)).padStart(2, '0')}`;
}

export class View {
  /** index into TIME_DIVS */
  divIndex = $state(7);
  live = $state(true);
  /** right edge while on HOLD, s */
  end = $state(0);
  selection = $state<[number, number] | null>(null);

  get timeDiv() {
    return TIME_DIVS[this.divIndex];
  }
  get windowS() {
    return this.timeDiv * DIVS_X;
  }

  /** The span on screen, kept inside the history. */
  span(store: PointStore): [number, number] {
    const w = this.windowS;
    let end = this.live ? store.tEnd : this.end;
    end = Math.min(end, store.tEnd);
    end = Math.max(end, Math.min(store.tStart + w, store.tEnd));
    return [end - w, end];
  }

  hold(store: PointStore) {
    if (this.live) {
      this.end = store.tEnd;
      this.live = false;
    }
  }

  /** Puts the right edge at `end`; at the newest point that is LIVE again. */
  moveTo(store: PointStore, end: number) {
    const right = Math.min(end, store.tEnd);
    if (right >= store.tEnd - this.windowS * 0.01) {
      this.live = true;
    } else {
      this.live = false;
      this.end = Math.max(right, store.tStart + this.windowS);
    }
  }

  zoom(store: PointStore, steps: number, anchor?: number) {
    const i = Math.max(0, Math.min(TIME_DIVS.length - 1, this.divIndex + steps));
    if (i === this.divIndex) return;
    if (!this.live && anchor !== undefined) {
      // keep the anchor time at the same place on screen
      const [a, b] = this.span(store);
      const frac = (anchor - a) / (b - a);
      const w = TIME_DIVS[i] * DIVS_X;
      this.divIndex = i;
      this.moveTo(store, anchor + (1 - frac) * w);
      return;
    }
    this.divIndex = i;
  }

  pan(store: PointStore, fraction: number) {
    const [, b] = this.span(store);
    this.moveTo(store, b + fraction * this.windowS);
  }
}
