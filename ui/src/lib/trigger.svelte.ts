// Trigger, display mode and event log, as on an oscilloscope.
//
// The core detects on every sample (trigger.rs): edge, window, pulse width
// or timeout on I / V / P, optionally smoothed. Every firing is an event
// (with width, peak, mean and integral for the timed kinds), paged into
// the event log here. The display mode decides what the screen shows:
//   roll    free-running: the newest data
//   auto    the latest trigger; free-runs when none comes for a while
//   normal  the latest trigger; holds the last one while none comes
//   single  the first trigger after arming, then stop
// A trigger is shown once the data after it fills the screen: it sits at
// `position` (the fraction of the screen before it).

import type { Link } from './device';
import type { Signal, TrigEvent, TriggerCfg, TriggerKind } from './nbp';
import type { SampleStore } from './store';
import type { View } from './view.svelte';

export type TrigMode = 'roll' | 'auto' | 'normal' | 'single';
export type TrigStatus = 'roll' | 'wait' | 'trigd' | 'auto' | 'stop';
export type KindType = TriggerKind['type'];

/** Auto free-runs after this long without a trigger (or 1.5 screens, if longer). */
const AUTO_TIMEOUT_MS = 300;
/** TRIG'D shows this long after the last one, then WAIT. */
const TRIGD_MS = 500;
/** the UI keeps this many events (the core keeps 100 000) */
const MAX_EVENTS = 100_000;
const PAGE = 5000;

export const KIND_LABEL: Record<KindType, string> = {
  edge: 'Edge',
  window: 'Window',
  pulse: 'Pulse width',
  timeout: 'Timeout',
};

export const EVENT_LABEL: Record<TrigEvent['what'], string> = {
  e: '↑ rise',
  f: '↓ fall',
  n: '→ enter',
  x: '← exit',
  p: '⊓ pulse',
  t: '⌛ timeout',
};

export class TriggerState {
  mode = $state<TrigMode>('roll');
  /** detect and log events in Roll mode too */
  log = $state(false);
  source = $state<Signal>('i');
  kind = $state<KindType>('edge');
  slope = $state<'rise' | 'fall' | 'both'>('rise');
  /** A / V / W */
  level = $state(0.01);
  low = $state(0.005);
  high = $state(0.015);
  when = $state<'enter' | 'exit' | 'both'>('both');
  polarity = $state<'high' | 'low'>('high');
  cond = $state<'any' | 'gt' | 'lt' | 'within'>('any');
  /** s */
  minWidth = $state(0.001);
  maxWidth = $state(0.1);
  timeout = $state(0.1);
  hysteresis = $state(0);
  holdoff = $state(0);
  /** moving average, s */
  filter = $state(0);
  /** fraction of the screen before the trigger */
  position = $state(0.5);

  status = $state<TrigStatus>('roll');
  /** the trigger on screen, s */
  shown = $state<number | null>(null);
  count = $state(0);

  /** the event log (oldest first); not deeply reactive: `logVersion` says when it changed */
  events: TrigEvent[] = [];
  logVersion = $state(0);
  private nextSeq = 0;
  private fetching = false;

  /** only triggers after this are used (mode change, arming) */
  private after = -Infinity;
  private lastShownWall = 0;

  get detecting() {
    return this.mode !== 'roll' || this.log;
  }

  kindCfg(): TriggerKind {
    switch (this.kind) {
      case 'edge':
        return { type: 'edge', slope: this.slope, level: this.level };
      case 'window':
        return { type: 'window', low: Math.min(this.low, this.high), high: Math.max(this.low, this.high), when: this.when };
      case 'pulse':
        return { type: 'pulse', polarity: this.polarity, level: this.level, cond: this.cond, min: this.minWidth, max: this.maxWidth };
      case 'timeout':
        return { type: 'timeout', polarity: this.polarity, level: this.level, time: this.timeout };
    }
  }

  cfg(): TriggerCfg {
    return {
      enabled: this.detecting,
      source: this.source,
      kind: this.kindCfg(),
      hysteresis: this.hysteresis,
      holdoff: this.holdoff,
      filter: this.filter,
    };
  }

  /** the level lines to draw on the source's plot */
  levels(): number[] {
    return this.kind === 'window' ? [this.low, this.high] : [this.level];
  }

  /** New settings went to the core: its log starts over, so does ours. */
  resetLog() {
    this.events = [];
    this.nextSeq = 0;
    this.logVersion++;
  }

  async clearLog(link: Link | null) {
    await link?.clearEvents();
    this.events = [];
    this.logVersion++;
  }

  /** Pages in events the core has and we don't. */
  async fetch(link: Link | null, count: number) {
    if (!link || this.fetching || count <= this.nextSeq) return;
    this.fetching = true;
    try {
      const page = await link.events(this.nextSeq, PAGE);
      if (page.length) {
        this.events.push(...page);
        if (this.events.length > MAX_EVENTS) this.events.splice(0, this.events.length - MAX_EVENTS);
        this.nextSeq = page[page.length - 1].seq + 1;
        this.logVersion++;
      } else {
        // the core dropped them (its log is full): skip ahead
        this.nextSeq = count;
      }
    } catch (e) {
      console.warn('events', e);
    } finally {
      this.fetching = false;
    }
  }

  /** Starts over from now: a new mode, new settings, or Single armed. */
  rearm(store: SampleStore, view: View) {
    this.after = store.tEnd;
    this.shown = null;
    this.lastShownWall = performance.now();
    if (this.mode === 'roll') view.anchor = null;
    if (this.mode === 'normal' || this.mode === 'single') {
      // nothing triggered yet: freeze where we are
      view.anchor = store.empty ? null : store.tEnd;
    }
    view.live = true;
    this.status = this.mode === 'roll' ? 'roll' : 'wait';
  }

  /** New trigger times from Meta: decide what to show. */
  update(recent: number[], count: number, store: SampleStore, view: View) {
    this.count = count;
    if (this.mode === 'roll') {
      view.anchor = null;
      this.status = 'roll';
      return;
    }
    if (!view.live) {
      if (this.status !== 'stop') this.status = 'stop';
      return;
    }
    const w = view.windowS;
    const post = (1 - this.position) * w;
    let pick: number | null = null;
    for (const t of recent) if (t > this.after && t > (this.shown ?? -Infinity) && t + post <= store.tEnd) pick = t;
    const now = performance.now();
    if (pick != null) {
      this.shown = pick;
      this.lastShownWall = now;
      view.anchor = pick + post;
      this.status = 'trigd';
      if (this.mode === 'single') {
        view.end = view.anchor;
        view.live = false;
        this.status = 'stop';
      }
      return;
    }
    const idle = now - this.lastShownWall;
    if (this.mode === 'auto' && idle > Math.max(AUTO_TIMEOUT_MS, 1500 * w)) {
      view.anchor = null;
      this.shown = null;
      this.status = 'auto';
    } else if (idle > TRIGD_MS && this.status === 'trigd') {
      this.status = 'wait';
    }
  }
}
