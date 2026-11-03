// Trigger and display mode, as on an oscilloscope.
//
// The core detects edges on every sample (trigger.rs) and reports the latest
// trigger times in Meta. Here we decide what the screen shows:
//   roll    free-running: the newest data (no trigger)
//   auto    the latest trigger; free-runs when none comes for a while
//   normal  the latest trigger; holds the last one while none comes
//   single  the first trigger after arming, then stop
// A trigger is shown once the data after it fills the screen: it sits at
// `position` (the fraction of the screen before it).

import type { Signal, TriggerCfg } from './nbp';
import type { SampleStore } from './store';
import type { View } from './view.svelte';

export type TrigMode = 'roll' | 'auto' | 'normal' | 'single';
export type TrigStatus = 'roll' | 'wait' | 'trigd' | 'auto' | 'stop';

/** Auto free-runs after this long without a trigger (or 1.5 screens, if longer). */
const AUTO_TIMEOUT_MS = 300;
/** TRIG'D shows this long after the last one, then WAIT. */
const TRIGD_MS = 500;

export class TriggerState {
  mode = $state<TrigMode>('roll');
  source = $state<Signal>('i');
  slope = $state<TriggerCfg['slope']>('rise');
  /** A / V / W */
  level = $state(0.01);
  hysteresis = $state(0);
  /** s */
  holdoff = $state(0);
  /** fraction of the screen before the trigger */
  position = $state(0.5);

  status = $state<TrigStatus>('roll');
  /** the trigger on screen, s */
  shown = $state<number | null>(null);
  count = $state(0);

  /** only triggers after this are used (mode change, arming) */
  private after = -Infinity;
  private lastShownWall = 0;

  cfg(): TriggerCfg {
    return {
      enabled: this.mode !== 'roll',
      source: this.source,
      slope: this.slope,
      level: this.level,
      hysteresis: this.hysteresis,
      holdoff: this.holdoff,
    };
  }

  /** Starts over from now: a new mode, new settings, or Single armed. */
  rearm(store: SampleStore, view: View) {
    this.after = store.tEnd;
    this.shown = null;
    this.lastShownWall = performance.now();
    view.anchor = this.mode === 'roll' ? null : view.anchor;
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
      // held by the user (or Single done): leave the screen alone
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
      view.anchor = null; // free-run
      this.shown = null;
      this.status = 'auto';
    } else if (idle > TRIGD_MS && this.status === 'trigd') {
      this.status = 'wait';
    }
  }
}
