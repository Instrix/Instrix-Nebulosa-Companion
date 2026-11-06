// Trigger, display mode and event log, as on an oscilloscope.
//
// The core detects on every sample (trigger.rs): edge, window, pulse (by
// width, charge, peak or mean), timeout, runt, slew (rise / fall time) or
// interval on I / V / P / math, optionally smoothed; then, if set, a second
// stage (A then B), a qualifier (only while another signal is above / below
// a level), holdoff, every Nth. Every firing is an event (with width, peak,
// mean and integral for the timed kinds), paged into the event log here.
// Or the Script type (the desktop app): the core detects nothing, the
// samples go in blocks to the user's Python function, and the triggers it
// finds come back into the log (see pumpScript). The display mode decides
// what the screen shows:
//   roll    free-running: the newest data
//   auto    the latest trigger; free-runs when none comes for a while
//   normal  the latest trigger; holds the last one while none comes
//   single  the first trigger after arming, then stop
// A trigger is shown once the data after it fills the screen: it sits at
// `position` (the fraction of the screen before it).

import { isNative, type Link } from './device';
import type { Signal, TrigEvent, TriggerCfg, TriggerKind, WidthCond } from './nbp';
import { scriptLoad, scriptReset, scriptScan, type ScriptInfo } from './script';
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
  pulse: 'Pulse',
  timeout: 'Timeout',
  runt: 'Runt',
  slew: 'Rise / fall time',
  interval: 'Interval',
  script: 'Script (Python)',
};

export const MEASURE_LABEL: Record<Measure, string> = {
  width: 'Width',
  charge: 'Charge ∫',
  peak: 'Peak',
  mean: 'Mean',
};
export type Measure = 'width' | 'charge' | 'peak' | 'mean';

export const EVENT_LABEL: Record<TrigEvent['what'], string> = {
  e: '↑ rise',
  f: '↓ fall',
  n: '→ enter',
  x: '← exit',
  p: '⊓ pulse',
  t: '⌛ timeout',
  r: '⊓ runt',
  u: '↗ rise time',
  d: '↘ fall time',
  i: '↔ interval',
  c: '✎ script',
};

/** the script's starting point */
export const SCRIPT_EXAMPLE = `# A trigger of your own. trigger(data) gets the new samples as they come,
# a block at a time, and returns where the triggers are: a list of times
# (s), or (time, label) pairs; labels show in the event list and the CSV.
# Or dicts: {"t": .., "label": .., "width": .., "peak": .., "mean": .., "integral": ..}
#
#   data.t   times (s)          data.i  current (A)    data.v  voltage (V)
#   data.p   power (W)          data.m  math channel (None without one)
#   data.dt  s per sample       data.runs  [(t0, count)]: stretches without a gap
#
# numpy arrays (np) if numpy is installed. Globals live on between blocks:
# keep what you need across the edge of a block. reset(), if there is one,
# runs when the trigger starts over.
#
# This one: a current burst over 20 mA whose voltage dips by more than
# 15 mV below the block's median, labelled with the burst's peak.

LEVEL = 0.020   # A
DIP = 0.015     # V
last = None     # the previous block's last current sample

def reset():
    global last
    last = None

def trigger(data):
    global last
    i, v = data.i, data.v
    prev = np.concatenate(([i[0] if last is None else last], i[:-1]))
    last = i[-1]
    starts = np.flatnonzero((prev < LEVEL) & (i >= LEVEL))
    found = []
    for k in starts:
        burst = slice(k, k + int(1e-3 / data.dt))       # the next 1 ms
        if np.median(v) - v[burst].min() > DIP:
            found.append((data.t[k], f"{i[burst].max() * 1e3:.1f} mA"))
    return found
`;

/** samples per block, at most */
const MAX_BLOCK = 500_000;
/** further behind than this (s) and the script skips ahead: it can't keep up */
const MAX_BEHIND = 2;

export interface ScriptRun {
  /** Python's and numpy's versions, once loaded */
  info: ScriptInfo | null;
  /** the last error (loading, or a traceback) */
  error: string;
  /** blocks and samples scanned, triggers found, blocks that raised */
  blocks: number;
  samples: number;
  found: number;
  failed: number;
  /** s of samples skipped: the script fell behind */
  skipped: number;
  /** s behind the newest sample */
  behind: number;
  /** ms per million samples, the latest block */
  msPerM: number;
  /** what it printed, the latest */
  out: string;
}

const SAVE_KEY = 'nebulosa.trigger';
/** the settings kept across sessions */
const SAVED = [
  // 'mode' and 'log' are not kept: the app always starts in Roll (free-running)
  'source', 'kind', 'slope', 'level', 'low', 'high', 'when', 'polarity', 'cond',
  'minWidth', 'maxWidth', 'timeout', 'hysteresis', 'holdoff', 'filter', 'position',
  'measure', 'minVal', 'maxVal', 'every', 'qualOn', 'qualSource', 'qualPolarity', 'qualLevel',
  'seqOn', 'seqSource', 'seqSlope', 'seqLevel', 'seqDelay', 'seqWithin', 'scriptCode',
] as const;
/** kept here only, not in a saved config: a config from someone else shouldn't name a program to run */
const LOCAL_ONLY = ['python'] as const;

export class TriggerState {
  mode = $state<TrigMode>('roll');
  /** detect and log events in Roll mode too */
  log = $state(false);
  /** the Logger's triggered capture is watching or armed: detect, whatever the Scope's mode */
  watch = $state(false);
  source = $state<Signal>('i');
  kind = $state<KindType>('edge');
  slope = $state<'rise' | 'fall' | 'both'>('rise');
  /** A / V / W */
  level = $state(0.01);
  low = $state(0.005);
  high = $state(0.015);
  when = $state<'enter' | 'exit' | 'both'>('both');
  polarity = $state<'high' | 'low'>('high');
  cond = $state<WidthCond>('any');
  /** s: pulse width, rise / fall time, interval */
  minWidth = $state(0.001);
  maxWidth = $state(0.1);
  /** a pulse's measure; the limits for charge (unit·s), peak and mean (unit) */
  measure = $state<Measure>('width');
  minVal = $state(1e-6);
  maxVal = $state(1e-3);
  /** only every Nth trigger */
  every = $state(1);
  /** only while another signal is above / below a level */
  qualOn = $state(false);
  qualSource = $state<Signal>('v');
  qualPolarity = $state<'high' | 'low'>('low');
  qualLevel = $state(3);
  /** A then B: B is an edge on its own signal, `seqDelay` to `seqWithin` after A (0: any time) */
  seqOn = $state(false);
  seqSource = $state<Signal>('i');
  seqSlope = $state<'rise' | 'fall' | 'both'>('rise');
  seqLevel = $state(0.01);
  seqDelay = $state(0);
  seqWithin = $state(0);
  /** the Script type (Python, desktop): its code (applied) */
  scriptCode = $state(SCRIPT_EXAMPLE);
  /** the interpreter (empty: python3, or python on Windows) */
  python = $state('');
  script = $state<ScriptRun>({ info: null, error: '', blocks: 0, samples: 0, found: 0, failed: 0, skipped: 0, behind: 0, msPerM: 0, out: '' });
  /** where the script's next block starts, s (null: from the newest sample) */
  private scanFrom: number | null = null;
  private pumping = false;
  /** the code that failed to load: wait for a change before trying again */
  private broken: string | null = null;
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
  /** pulse widths of `events`, kept as they arrive (the summary reads these, not every event) */
  widths = { n: 0, sum: 0, min: Infinity, max: -Infinity };
  private nextSeq = 0;
  private fetching = false;

  /** only triggers after this are used (mode change, arming) */
  private after = -Infinity;
  private lastShownWall = 0;

  constructor() {
    try {
      const v = JSON.parse(localStorage.getItem(SAVE_KEY) ?? 'null');
      if (v && typeof v === 'object')
        for (const k of [...SAVED, ...LOCAL_ONLY]) if (k in v && typeof v[k] === typeof this[k]) (this as Record<string, unknown>)[k] = v[k];
    } catch {
      /* storage unavailable */
    }
  }

  /** the full settings (including mode / log) for a saved config */
  dump(): Record<string, unknown> {
    const keys = [...SAVED, 'mode', 'log'] as const;
    return Object.fromEntries(keys.map((k) => [k, this[k as keyof this]]));
  }

  /** apply settings from a config (only keys of a matching type) */
  restore(v: Record<string, unknown>) {
    // not python: a config from elsewhere can't choose a program to run
    const keys = [...SAVED, 'mode', 'log'];
    for (const k of keys) if (k in v && typeof v[k] === typeof (this as Record<string, unknown>)[k]) (this as Record<string, unknown>)[k] = v[k];
  }

  /** Saves the settings (call from an effect: it reads them all). */
  save() {
    const v = Object.fromEntries([...SAVED, ...LOCAL_ONLY].map((k) => [k, this[k]]));
    try {
      localStorage.setItem(SAVE_KEY, JSON.stringify(v));
    } catch {
      /* storage unavailable */
    }
  }

  get detecting() {
    return this.mode !== 'roll' || this.log || this.watch;
  }

  kindCfg(): TriggerKind {
    const lo = Math.min(this.low, this.high), hi = Math.max(this.low, this.high);
    switch (this.kind) {
      case 'edge':
        return { type: 'edge', slope: this.slope, level: this.level };
      case 'window':
        return { type: 'window', low: Math.min(this.low, this.high), high: Math.max(this.low, this.high), when: this.when };
      case 'pulse': {
        const w = this.measure === 'width';
        return { type: 'pulse', polarity: this.polarity, level: this.level, cond: this.cond, min: w ? this.minWidth : this.minVal, max: w ? this.maxWidth : this.maxVal, measure: this.measure };
      }
      case 'timeout':
        return { type: 'timeout', polarity: this.polarity, level: this.level, time: this.timeout };
      case 'runt':
        return { type: 'runt', polarity: this.polarity, low: lo, high: hi };
      case 'slew':
        return { type: 'slew', slope: this.slope, low: lo, high: hi, cond: this.cond, min: this.minWidth, max: this.maxWidth };
      case 'interval':
        return { type: 'interval', slope: this.slope, level: this.level, cond: this.cond, min: this.minWidth, max: this.maxWidth };
      case 'script':
        return { type: 'script' };
    }
  }


  cfg(): TriggerCfg {
    if (this.kind === 'script')
      // the script does it all: nothing of the core's on top
      return { enabled: this.detecting && isNative, source: this.source, kind: { type: 'script' }, hysteresis: 0, holdoff: 0, filter: 0, every: 1, qual: null, seq: null };
    return {
      enabled: this.detecting,
      source: this.source,
      kind: this.kindCfg(),
      hysteresis: this.hysteresis,
      holdoff: this.holdoff,
      filter: this.filter,
      every: Math.max(1, Math.round(this.every) || 1),
      qual: this.qualOn ? { source: this.qualSource, polarity: this.qualPolarity, level: this.qualLevel } : null,
      seq: this.seqOn
        ? { source: this.seqSource, kind: { type: 'edge', slope: this.seqSlope, level: this.seqLevel }, delay: this.seqDelay, within: this.seqWithin }
        : null,
    };
  }

  /** the extras on top of the detector, in a few words (or '' for none) */
  extras(): string {
    if (this.kind === 'script') return '';
    const x: string[] = [];
    if (this.seqOn) x.push('then B');
    if (this.qualOn) x.push('qualified');
    if (this.every > 1) x.push(`every ${Math.round(this.every)}`);
    return x.join(', ');
  }

  /** two levels (low, high) rather than one */
  get banded() {
    return this.kind === 'window' || this.kind === 'runt' || this.kind === 'slew';
  }

  /** the level lines to draw on the source's plot */
  levels(): number[] {
    return this.kind === 'script' ? [] : this.banded ? [this.low, this.high] : [this.level];
  }

  /** New settings went to the core: its log starts over, so does ours. */
  resetLog() {
    this.scanFrom = null;
    this.script = { ...this.script, blocks: 0, samples: 0, found: 0, failed: 0, skipped: 0, behind: 0, error: this.broken ? this.script.error : '' };
    if (this.kind === 'script' && isNative) void scriptReset(this.python, this.scriptCode).catch(() => undefined);
    this.events = [];
    this.countWidths();
    this.nextSeq = 0;
    this.logVersion++;
  }

  async clearLog(link: Link | null) {
    await link?.clearEvents();
    this.events = [];
    this.countWidths();
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
        if (this.events.length > MAX_EVENTS) {
          this.events.splice(0, this.events.length - MAX_EVENTS);
          this.countWidths(); // a min or max may have gone: count again (once per page, at the cap)
        } else this.countWidths(page);
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

  /** adds `list` to the width totals; without it, counts all `events` again */
  private countWidths(list?: TrigEvent[]) {
    const w = this.widths;
    if (!list) {
      Object.assign(w, { n: 0, sum: 0, min: Infinity, max: -Infinity });
      list = this.events;
    }
    for (const e of list)
      if (e.width != null && isFinite(e.width)) {
        w.n++;
        w.sum += e.width;
        if (e.width < w.min) w.min = e.width;
        if (e.width > w.max) w.max = e.width;
      }
  }

  /** New settings: only triggers found with them count; the screen stays where it is. */
  settingsChanged(store: SampleStore) {
    this.after = store.tEnd;
    if (this.status === 'trigd') this.status = 'wait';
  }

  /** Starts over from now: a new mode, or Single armed. */
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

  /** The script: load it now (a check, and to show Python's version), or say why it won't. */
  async loadScript(): Promise<boolean> {
    try {
      this.script.info = await scriptLoad(this.python, this.scriptCode);
      this.script.error = '';
      this.broken = null;
      return true;
    } catch (e) {
      this.script.info = null;
      this.script.error = String(e);
      this.broken = this.python + '\0' + this.scriptCode;
      return false;
    }
  }

  /**
   * The Script type's turn (each Meta while it detects): the samples since
   * the last block (from `tStart` to `tEnd`, what the buffer holds), in
   * blocks, to the script, for a moment at most; the triggers it finds go
   * into the log. Too far behind, it skips ahead.
   */
  async pumpScript(link: Link | null, tStart: number, tEnd: number) {
    if (!link || this.pumping || this.kind !== 'script' || !this.detecting || !isNative) return;
    if (this.broken === this.python + '\0' + this.scriptCode) return;
    this.pumping = true;
    const r = this.script;
    const code = this.scriptCode, python = this.python;
    try {
      let from = this.scanFrom ?? tEnd;
      if (from < tStart) from = tStart; // a new stream, or the buffer moved on
      const until = performance.now() + 250;
      while (from < tEnd && performance.now() < until) {
        if (tEnd - from > MAX_BEHIND) {
          r.skipped += tEnd - MAX_BEHIND - from;
          from = tEnd - MAX_BEHIND;
        }
        const block = link.nativePipeline ? null : await link.sampleBlock(from, tEnd, MAX_BLOCK);
        const res = await scriptScan(python, code, from, tEnd, MAX_BLOCK, block);
        if (code !== this.scriptCode || this.kind !== 'script') break; // changed meanwhile: what it found is stale
        if (!res.n) break;
        from = res.next;
        r.blocks++;
        r.samples += res.n;
        r.found += res.found;
        r.msPerM = (res.ms / res.n) * 1e6;
        if (res.out) r.out = (r.out + res.out).slice(-4000);
        if (res.error) {
          r.failed++;
          r.error = res.error;
        }
        if (res.events.length)
          await link.injectEvents(
            res.events.map((e, k) => ({ seq: k, t: e.t, what: 'c', width: e.width, peak: e.peak, mean: e.mean, integral: e.integral, label: e.label ?? undefined })),
          );
      }
      this.scanFrom = from;
      r.behind = Math.max(0, tEnd - from);
      if (!r.info) r.info = await scriptLoad(python, code).catch(() => null);
    } catch (err) {
      // it didn't load, it died or it hung: say so, and wait for a change before trying again
      r.error = String(err);
      r.failed++;
      this.broken = python + '\0' + code;
    } finally {
      this.pumping = false;
    }
  }
}
