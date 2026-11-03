// What the scope screen shows: the time window (LIVE follows the newest
// point; HOLD stays where it was put) and the markers. Shared by the plots,
// the overview and the meter.
//
// Markers: single (one time) or dual (a span: Δt, statistics, integrals).
// One marker is active; `selection` is the active dual marker's span.

import type { SampleStore } from './store';

/** time/div steps, s (1-2-5) */
export const TIME_DIVS = [
  1e-6, 2e-6, 5e-6, 1e-5, 2e-5, 5e-5, 1e-4, 2e-4, 5e-4, 1e-3, 2e-3, 5e-3, 0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1, 2, 5,
  10, 20, 30, 60,
];
export const DIVS_X = 10;
export const DIVS_Y = 8;

export function fmtTime(s: number, digits = 3): string {
  const a = Math.abs(s);
  if (a === 0) return '0';
  if (a < 1e-6) return `${+(s * 1e9).toPrecision(digits)} ns`;
  if (a < 1e-3) return `${+(s * 1e6).toPrecision(digits)} µs`;
  if (a < 1) return `${+(s * 1e3).toPrecision(digits)} ms`;
  if (a < 60) return `${+s.toPrecision(digits)} s`;
  const m = Math.floor(a / 60);
  return `${s < 0 ? '-' : ''}${m}:${String(Math.round(a - m * 60)).padStart(2, '0')}`;
}

export interface Marker {
  id: number;
  /** s */
  t0: number;
  /** s; null for a single marker */
  t1: number | null;
}

export const MARKER_COLORS = ['#ff4fd8', '#7cf05a', '#5ab4ff', '#ffb547', '#c792ff', '#ff6b6b'];

export function markerColor(m: Marker) {
  return MARKER_COLORS[(m.id - 1) % MARKER_COLORS.length];
}

export function markerName(m: Marker) {
  return `${m.t1 == null ? 'M' : 'D'}${m.id}`;
}

export class View {
  /** index into TIME_DIVS */
  divIndex = $state(TIME_DIVS.indexOf(0.1));
  live = $state(true);
  /** right edge while on HOLD, s */
  end = $state(0);
  markers = $state<Marker[]>([]);
  active = $state<number | null>(null);
  private nextId = 1;

  /** the active dual marker's span; setting a span adds a dual marker, null deselects */
  get selection(): [number, number] | null {
    const m = this.markers.find((m) => m.id === this.active);
    return m && m.t1 != null ? [m.t0, m.t1] : null;
  }
  set selection(v: [number, number] | null) {
    if (v) this.addDual(v[0], v[1]);
    else this.active = null;
  }

  get activeMarker(): Marker | undefined {
    return this.markers.find((m) => m.id === this.active);
  }

  addDual(a: number, b: number): Marker {
    const m = { id: this.nextId++, t0: Math.min(a, b), t1: Math.max(a, b) };
    this.markers.push(m);
    this.active = m.id;
    return m;
  }

  addSingle(t: number): Marker {
    const m = { id: this.nextId++, t0: t, t1: null };
    this.markers.push(m);
    this.active = m.id;
    return m;
  }

  remove(id: number | null) {
    this.markers = this.markers.filter((m) => m.id !== id);
    if (this.active === id) this.active = null;
  }

  clearMarkers() {
    this.markers = [];
    this.active = null;
    this.nextId = 1;
  }

  /**
   * Moves an edge (0 = t0, 1 = t1) to t, or with 'both' the whole marker so
   * t0 = t. Returns the edge being dragged afterwards: dragging one edge past
   * the other swaps them.
   */
  moveMarker(id: number, edge: 0 | 1 | 'both', t: number): 0 | 1 | 'both' {
    const m = this.markers.find((m) => m.id === id);
    if (!m) return edge;
    if (edge === 'both') {
      const d = t - m.t0;
      m.t0 += d;
      if (m.t1 != null) m.t1 += d;
    } else if (edge === 0) {
      m.t0 = t;
    } else {
      m.t1 = t;
    }
    if (m.t1 != null && m.t1 < m.t0) {
      [m.t0, m.t1] = [m.t1, m.t0];
      if (edge !== 'both') return edge === 0 ? 1 : 0;
    }
    return edge;
  }

  get timeDiv() {
    return TIME_DIVS[this.divIndex];
  }
  get windowS() {
    return this.timeDiv * DIVS_X;
  }

  /** The span on screen, kept inside the history. */
  span(store: SampleStore): [number, number] {
    const w = this.windowS;
    let end = this.live ? store.tEnd : this.end;
    end = Math.min(end, store.tEnd);
    end = Math.max(end, Math.min(store.tStart + w, store.tEnd));
    return [end - w, end];
  }

  hold(store: SampleStore) {
    if (this.live) {
      this.end = store.tEnd;
      this.live = false;
    }
  }

  /** Puts the right edge at `end`; at the newest point that is LIVE again. */
  moveTo(store: SampleStore, end: number) {
    const right = Math.min(end, store.tEnd);
    if (right >= store.tEnd - this.windowS * 0.01) {
      this.live = true;
    } else {
      this.live = false;
      this.end = Math.max(right, store.tStart + this.windowS);
    }
  }

  zoom(store: SampleStore, steps: number, anchor?: number) {
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

  pan(store: SampleStore, fraction: number) {
    const [, b] = this.span(store);
    this.moveTo(store, b + fraction * this.windowS);
  }
}
