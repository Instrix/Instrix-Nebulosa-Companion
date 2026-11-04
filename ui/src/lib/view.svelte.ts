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

/**
 * A duration typed by hand, s: `90`, `90s`, `500ms`, `2.5 min`, `1m30s`,
 * `1h`, `1h 20m`, `2:05` (m:ss), `1:02:03` (h:mm:ss). null if it isn't one.
 */
export function parseDuration(text: string): number | null {
  const v = text.trim().toLowerCase().replace(/\s+/g, '');
  if (!v) return null;
  const colon = v.match(/^(?:(\d+):)?(\d+):(\d{1,2}(?:\.\d+)?)$/);
  if (colon) return Number(colon[1] ?? 0) * 3600 + Number(colon[2]) * 60 + Number(colon[3]);
  const unit: Record<string, number> = { ms: 1e-3, s: 1, sec: 1, m: 60, min: 60, h: 3600, hr: 3600, d: 86400 };
  let total = 0;
  let rest = v;
  while (rest) {
    const m = rest.match(/^(\d+(?:\.\d+)?|\.\d+)(ms|sec|s|min|m|hr|h|d)?/);
    if (!m) return null;
    total += Number(m[1]) * (m[2] ? unit[m[2]] : 1);
    rest = rest.slice(m[0].length);
    if (!m[2] && rest) return null; // a bare number only on its own
  }
  return isFinite(total) ? total : null;
}

/** a duration for reading and typing back: 500 ms, 30 s, 1 min 30 s, 2 h 5 min, 27.8 h */
export function fmtDur(s: number): string {
  if (s < 1) return `${+(s * 1000).toPrecision(3)} ms`;
  if (s < 60) return `${+s.toPrecision(3)} s`;
  if (s < 3600) {
    const m = Math.floor(s / 60), r = Math.round(s - m * 60);
    return r ? `${m} min ${r} s` : `${m} min`;
  }
  if (s < 10 * 3600) {
    const h = Math.floor(s / 3600), m = Math.round((s - h * 3600) / 60);
    return m ? `${h} h ${m} min` : `${h} h`;
  }
  return `${+(s / 3600).toFixed(1)} h`;
}

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
  /** a name the user gave it, else the default Mn / Dn */
  name?: string;
}

export const MARKER_COLORS = ['#ff4fd8', '#7cf05a', '#5ab4ff', '#ffb547', '#c792ff', '#ff6b6b'];

export function markerColor(m: Marker) {
  return MARKER_COLORS[(m.id - 1) % MARKER_COLORS.length];
}

export function markerName(m: Marker) {
  return m.name?.trim() || `${m.t1 == null ? 'M' : 'D'}${m.id}`;
}

export class View {
  /** index into TIME_DIVS */
  divIndex = $state(TIME_DIVS.indexOf(0.1));
  live = $state(true);
  /** right edge while on HOLD, s */
  end = $state(0);
  /** right edge set by the trigger while LIVE (null: follow the newest sample) */
  anchor = $state<number | null>(null);
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

  /** rename a marker (empty clears the custom name) */
  setName(id: number, name: string) {
    const m = this.markers.find((x) => x.id === id);
    if (m) {
      m.name = name.trim() || undefined;
      this.markers = [...this.markers];
    }
  }

  /** put back a set of markers (undo a clear) */
  restore(markers: Marker[]) {
    this.markers = markers.map((m) => ({ ...m }));
    this.nextId = Math.max(0, ...markers.map((m) => m.id)) + 1;
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
    let end = this.live ? (this.anchor ?? store.tEnd) : this.end;
    end = Math.min(end, store.tEnd);
    end = Math.max(end, Math.min(store.tStart + w, store.tEnd));
    return [end - w, end];
  }

  hold(store: SampleStore) {
    if (this.live) {
      this.end = this.span(store)[1];
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

  /** Shows [a, b]: the smallest time/div that fits it, centred on it, on HOLD. */
  zoomTo(store: SampleStore, a: number, b: number) {
    if (!(b > a)) return;
    let i = TIME_DIVS.findIndex((d) => d * DIVS_X >= b - a);
    if (i < 0) i = TIME_DIVS.length - 1;
    this.divIndex = i;
    const w = TIME_DIVS[i] * DIVS_X;
    // held even at the newest data: a zoom is to look at something
    this.anchor = null;
    this.live = false;
    this.end = Math.max(Math.min((a + b) / 2 + w / 2, store.tEnd), Math.min(store.tStart + w, store.tEnd));
  }

  pan(store: SampleStore, fraction: number) {
    const [, b] = this.span(store);
    this.moveTo(store, b + fraction * this.windowS);
  }
}
