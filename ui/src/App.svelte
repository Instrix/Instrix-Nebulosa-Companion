<script lang="ts">
  import { ask } from './lib/ask';
  import { onMount, tick, untrack } from 'svelte';
  import Scope, { SIGNAL_INFO, mathInfo, type AxisCfg, type ScopeSpot, type TraceCfg } from './lib/Scope.svelte';
  import ContextMenu from './lib/ContextMenu.svelte';
  import { openMenu, type MenuEntry } from './lib/menu.svelte';
  import Overview from './lib/Overview.svelte';
  import MarkerList from './lib/MarkerList.svelte';
  import Meter, { type AccumView, type MarkerRow, type Scope as MeterScope, type Summary } from './lib/Meter.svelte';
  import { View, TIME_DIVS, fmtTime, markerColor, markerName } from './lib/view.svelte';
  import { Moments, SampleStore, type SpanStats } from './lib/store';
  import { Device, isNative, type Candidate, type Link } from './lib/device';
  import {
    CAP,
    FPGA_ID_FORMAT5,
    SIGNALS,
    SRC,
    core,
    hex,
    type Accum,
    type Calibration,
    type DataStats,
    type Info,
    type Meta,
    type Signal,
    type TrigEvent,
  } from './lib/nbp';
  import { fmtSI, fmtRate, fmtBytes as fmtBytesSize } from './lib/si';
  import { siInput } from './lib/siInput';
  import * as native from './lib/native';
  import * as web from './lib/webusb';
  import { DemoLink, demoRequested, demoSelection } from './lib/demo';
  import { EVENT_LABEL, KIND_LABEL, TriggerState, type KindType, type TrigMode } from './lib/trigger.svelte';
  import EventList from './lib/EventList.svelte';
  import Analysis from './lib/Analysis.svelte';
  import Updater from './lib/Updater.svelte';
  import TriggerPanel from './lib/panels/TriggerPanel.svelte';
  import SignalsPanel from './lib/panels/SignalsPanel.svelte';
  import CalibrationPanel from './lib/panels/CalibrationPanel.svelte';
  import ConnectPanel from './lib/panels/ConnectPanel.svelte';
  import MathPanel from './lib/panels/MathPanel.svelte';
  import BoardPanel from './lib/panels/BoardPanel.svelte';
  import Section from './lib/panels/Section.svelte';
  import Help, { type HelpTab } from './lib/Help.svelte';
  import Tour, { markTourSeen, tourSeen, type TourStep } from './lib/Tour.svelte';
  import { SCALES, canScale, restoreScale, setScale, setTheme, stepScale, uiScale, uiTheme } from './lib/uiScale.svelte';
  import instrixIcon from './assets/instrix-icon.svg';
  import instrixIconWhite from './assets/instrix-icon-white.svg';
  import * as boardCmd from './lib/board';
  import LoggerView, { type LogView } from './lib/LoggerView.svelte';
  import { ViewHistory } from './lib/history.svelte';
  import CommandPalette, { type Command } from './lib/CommandPalette.svelte';
  import { SECTION_HELP } from './lib/panels/sectionHelp';
  import ExportEditor, { defaultOutput, type OutputCfg } from './lib/ExportEditor.svelte';
  import { drag, stored, store as save } from './lib/drag';
  import Showcases from './lib/Showcases.svelte';
  import SideChannel from './lib/SideChannel.svelte';
  import SectionNav from './lib/panels/SectionNav.svelte';
  import SerialList from './lib/SerialList.svelte';
  import ConfigDialog from './lib/ConfigDialog.svelte';
  import { CONFIG_VERSION, SECTION_IDS, listConfigs, defaultConfigName, asConfig, type Config, type SectionId } from './lib/config';

  import { SerialMonitor, defaultSerialConfig, type SerialConfig } from './lib/serialMonitor.svelte';
  import type { Showcase, ShowcaseCtx } from './lib/showcases';

  /** full-rate buffer sizes, MB (4 bytes per sample) */
  const BUFFER_CHOICES = [64, 128, 256, 512, 1024, 2048];
  const BUFFER_KEY = 'nebulosa.bufferMB';
  const CURRENT_RANGES = 6;

  // ---- connection ---------------------------------------------------------
  let candidates = $state<Candidate[]>([]);
  let scanning = $state(false);
  let busy = $state(false);
  let error = $state('');

  let dev = $state<Device | null>(null);
  let info = $state<Info | null>(null);
  let streaming = $state(false);

  // ---- acquisition --------------------------------------------------------
  let source = $state<number>(stored('nebulosa.source', SRC.STUB_RANDOM));
  let log2n = $state(4);
  let decimMode = $state(0);
  let rateHz = $state(0);
  let rangeAuto = $state(stored('nebulosa.rangeAuto', 1) === 1);
  let rangeManual = $state(stored('nebulosa.rangeManual', 0));
  let rangeMin = $state(stored('nebulosa.rangeMin', 0));
  let rangeMax = $state(stored('nebulosa.rangeMax', 5));

  // ---- data and view ------------------------------------------------------
  const store = new SampleStore();
  /** the buffer holds samples (reactive; the store itself isn't) */
  let hasData = $state(false);
  const view = new View();
  const trig = new TriggerState();
  function loadBufferMB(): number {
    try {
      const v = Number(localStorage.getItem(BUFFER_KEY));
      if (BUFFER_CHOICES.includes(v)) return v;
    } catch {
      /* storage unavailable */
    }
    return 256;
  }
  let bufferMB = $state(loadBufferMB());
  const bufferSamples = $derived((bufferMB * 1024 * 1024) / 4);
  /** what the buffer holds at the current rate, s */
  const bufferS = $derived(bufferSamples / (rateHz || 2e6));
  let shown = $state<Record<Signal, boolean>>({ i: true, v: true, p: true, m: true });
  /** the signals there are: the math channel only once it is set */
  const available = $derived(SIGNALS.filter((s) => s !== 'm' || mathInfo.on));
  let trace = $state<TraceCfg>({ mean: true, range: true, sigma: false });
  const signals = $derived(available.filter((s) => shown[s]));
  let axes = $state<Record<Signal, AxisCfg>>(loadAxes());
  function loadAxes(): Record<Signal, AxisCfg> {
    const base: Record<Signal, AxisCfg> = {
      i: { auto: true, min: 0, max: 0.1, log: false },
      v: { auto: true, min: 0, max: 5, log: false },
      p: { auto: true, min: 0, max: 0.5, log: false },
      m: { auto: true, min: 0, max: 1, log: false },
    };
    try {
      const v = JSON.parse(localStorage.getItem('nebulosa.axes') ?? 'null');
      if (v) for (const s of SIGNALS) if (v[s]) Object.assign(base[s], { auto: !!v[s].auto, log: !!v[s].log });
    } catch {
      /* storage unavailable */
    }
    return base;
  }
  // keep the per-signal log / fixed choice across sessions (not the dynamic min/max)
  $effect(() => {
    const flags = Object.fromEntries(SIGNALS.map((s) => [s, { auto: axes[s].auto, log: axes[s].log }]));
    save('nebulosa.axes', flags);
  });
  $effect(() => save('nebulosa.source', source));
  $effect(() => {
    save('nebulosa.rangeAuto', rangeAuto ? 1 : 0);
    save('nebulosa.rangeManual', rangeManual);
    save('nebulosa.rangeMin', rangeMin);
    save('nebulosa.rangeMax', rangeMax);
  });

  // ---- meter --------------------------------------------------------------
  let runMoments = $state.raw<Meta['run'] | null>(null);
  let meterScope = $state<MeterScope>('window');
  let hold = $state(false);
  let summary = $state<Summary | null>(null);
  let markerRows = $state<(MarkerRow & { t0: number })[]>([]);
  let accum: Accum | null = null;
  let accumBase: Accum = { charge: 0, energy: 0, seconds: 0 };
  let accumView = $state<AccumView | null>(null);
  let streamId = -1;

  // ---- files ------------------------------------------------------------
  let recording = $state<Meta['recording'] | null>(null);
  let recFiles = $state<native.RecFile[]>([]);
  let recDir = $state('');
  let note = $state('');
  let noteAction = $state<{ label: string; run: () => void } | null>(null);
  let noteHistory = $state<{ text: string; wall: number }[]>([]);
  let noteHistoryOpen = $state(false);
  /** show a transient note, with an optional action button and a kept history */
  function toast(text: string, action?: { label: string; run: () => void }) {
    noteAction = action ?? null;
    note = text; // the $effect records it in the history
  }
  // route plain note assignments to the history too
  $effect(() => {
    if (note && (!noteHistory.length || noteHistory[noteHistory.length - 1].text !== note)) {
      noteHistory = [...noteHistory.slice(-49), { text: note, wall: Date.now() }];
    }
  });
  /** a recording open for playback: its header */
  let playback = $state<Record<string, unknown> | null>(null);
  let fitPending = false;
  let scope = $state<Scope>();
  let logger = $state<LoggerView>();

  async function refreshRecordings() {
    if (!isNative) return;
    try {
      recFiles = await native.listRecordings();
      recDir = await native.recordingsDir();
    } catch (e) {
      console.warn('recordings', e);
    }
  }

  async function toggleRecording() {
    error = '';
    try {
      if (recording?.active) {
        const r = await native.stopRecording();
        if (r) note = `Saved ${r.path.split(/[\\/]/).pop()} · ${fmtBytesSize(r.bytes)} · ${fmtTime(r.seconds)}`;
        refreshRecordings();
      } else {
        const path = await native.startRecording({
          app: 'Nebulosa Companion',
          created: new Date().toISOString(),
          link: dev?.link.description,
          device: info,
          rate_hz: rateHz,
          decimation: { log2n, mode: decimMode },
          source,
          calibration: $state.snapshot(cal),
        });
        note = `Recording to ${path}`;
      }
    } catch (e) {
      error = String(e);
    }
  }

  async function openRecording(path: string) {
    busy = true;
    error = '';
    try {
      await disconnect();
      const isLog = path.endsWith('.nblog');
      const { link, header } = await (isLog ? native.openLog : native.openRecording)(path, {
        onMeta,
        onClosed: () => undefined,
      });
      if (typeof header.wall_offset === 'number') wallOffset = header.wall_offset;
      else if (typeof header.created === 'string') wallPending = Date.parse(header.created) / 1000;
      if (isLog) {
        setAppMode('logger');
        lv = { follow: true, last: 0, t0: 0, t1: 0 };
      }
      dev = new Device(link);
      playback = header;
      await linkMath(link);
      await link.setLogInterval(logInterval);
      store.attach(link);
      setLink(link);
      trig.mode = 'roll';
      clearData();
      fitPending = true;
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function openFile() {
    const p = await native.pickRecording();
    if (p) await openRecording(p);
  }

  /** the whole recording on screen */
  function fitAll() {
    if (store.empty) return;
    const len = store.tEnd - store.tStart;
    let k = TIME_DIVS.findIndex((d) => d * 10 >= len);
    if (k < 0) k = TIME_DIVS.length - 1;
    view.divIndex = k;
    view.live = false;
    view.end = store.tStart + view.windowS;
  }

  /** the span to export: the active dual marker, else the screen */
  function exportSpan(): [number, number] {
    return view.selection ?? view.span(store);
  }

  async function exportCsv() {
    error = '';
    try {
      const [t0, t1] = exportSpan();
      const n = await native.countSamples(t0, t1);
      if (!n) throw new Error('no samples in the span');
      if (n > 5e6 && !(await ask(`${(n / 1e6).toFixed(1)} M samples, about ${fmtBytesSize(n * 60)} of CSV. Export?`))) return;
      const path = await native.pickSave(`${recDir || '.'}/${stamp()}.csv`, 'CSV', 'csv');
      if (!path) return;
      const rows = await native.exportCsv(t0, t1, path);
      note = `Exported ${rows.toLocaleString()} samples to ${path.split(/[\\/]/).pop()}`;
    } catch (e) {
      error = String(e);
    }
  }

  async function exportPng() {
    error = '';
    try {
      if (!scope) throw new Error("the scope isn't shown");
      const canvas = scope.snapshot();
      const blob = await new Promise<Blob | null>((r) => canvas.toBlob(r, 'image/png'));
      if (!blob) throw new Error('no image');
      if (isNative) {
        const path = await native.pickSave(`${recDir || '.'}/${stamp()}.png`, 'PNG image', 'png');
        if (!path) return;
        await native.saveBytes(path, new Uint8Array(await blob.arrayBuffer()));
        note = `Saved ${path.split(/[\\/]/).pop()}`;
      } else {
        const a = document.createElement('a');
        a.href = URL.createObjectURL(blob);
        a.download = `nebulosa-${stamp()}.png`;
        a.click();
        setTimeout(() => URL.revokeObjectURL(a.href), 1000);
      }
    } catch (e) {
      error = String(e);
    }
  }

  function stamp() {
    return new Date().toISOString().slice(0, 19).replace(/[:T]/g, '-');
  }



  // ---- link counters ------------------------------------------------------
  let stats = $state<DataStats | null>(null);
  let throughput = $state(0);
  let lastBytes = { t: 0, bytes: 0 };

  // ---- calibration --------------------------------------------------------
  const CAL_KEY = 'nebulosa.calibration';
  function loadCal(): Calibration {
    try {
      const c = JSON.parse(localStorage.getItem(CAL_KEY) ?? 'null');
      if (c && Array.isArray(c.i_fs)) return c;
    } catch {
      /* storage unavailable */
    }
    return core.default_calibration();
  }
  let cal = $state<Calibration>(loadCal());
  let calNote = $state('');

  async function applyCal(c: Calibration = cal) {
    try {
      localStorage.setItem(CAL_KEY, JSON.stringify(c));
    } catch {
      /* storage unavailable */
    }
    if (dev) await dev.link.setCalibration($state.snapshot(c));
    calNote = dev ? 'Applied to new samples.' : 'Saved; applied when connected.';
  }

  function resetCal() {
    cal = core.default_calibration();
    applyCal();
  }

  // ---- trigger ------------------------------------------------------------
  let activeLink = $state.raw<Link | null>(null);
  function setLink(l: Link | null) {
    activeLink = l;
  }

  // any trigger setting change: to the core, and start over
  $effect(() => {
    const cfg = trig.cfg();
    activeLink?.setTrigger(cfg);
    // depends on the settings only, not on what rearm reads
    untrack(() => {
      trig.resetLog();
      trig.settingsChanged(store);
    });
  });

  // a new mode (or link) re-arms: the screen may freeze or start following
  $effect(() => {
    void trig.mode;
    void activeLink;
    untrack(() => trig.rearm(store, view));
  });

  // keep the trigger settings across sessions
  $effect(() => trig.save());

  function setMode(m: TrigMode) {
    trig.mode = m;
  }

  /** level halfway between the screen's min and max of the source */
  async function level50() {
    if (store.empty) return;
    const st = await store.stats(...view.span(store));
    const m = st[trig.source];
    if (!m.n) return;
    const mid = (m.min + m.max) / 2;
    trig.level = mid;
    const q = (m.max - m.min) / 4;
    trig.low = mid - q;
    trig.high = mid + q;
  }

  /** shows an event: at the trigger position, on HOLD */
  function jumpToEvent(e: TrigEvent) {
    view.hold(store);
    view.end = e.t + (1 - trig.position) * view.windowS;
  }

  async function exportEvents() {
    const head = 'seq,time_s,event,width_s,peak,mean,integral\n';
    const num = (x: number | null) => (x == null || !isFinite(x) ? '' : x.toExponential(6));
    const body = trig.events.map((e) => `${e.seq},${e.t.toFixed(9)},${e.what},${num(e.width)},${num(e.peak)},${num(e.mean)},${num(e.integral)}`).join('\n');
    const bytes = new TextEncoder().encode(head + body + '\n');
    try {
      if (isNative) {
        const path = await native.pickSave(`${recDir || '.'}/events-${stamp()}.csv`, 'CSV', 'csv');
        if (!path) return;
        await native.saveBytes(path, bytes);
        note = `Exported ${trig.events.length.toLocaleString()} events`;
      } else {
        const a = document.createElement('a');
        a.href = URL.createObjectURL(new Blob([bytes], { type: 'text/csv' }));
        a.download = `nebulosa-events-${stamp()}.csv`;
        a.click();
        setTimeout(() => URL.revokeObjectURL(a.href), 1000);
      }
    } catch (e) {
      error = String(e);
    }
  }

  let bottomTab = $state<'markers' | 'events' | 'serial'>('markers');
  let updating = $state(false);
  let helpOpen = $state(false);
  let helpTab = $state<HelpTab>('start');

  function openHelp(tab: HelpTab) {
    helpTab = tab;
    helpOpen = true;
  }

  function helpMenu(e: MouseEvent) {
    openMenu(e, [
      { label: 'Getting started', hint: '?', run: () => openHelp('start') },
      { label: 'Walkthrough', run: startTour },
      { label: 'Tutorials and showcases…', run: () => (showcasesOpen = true) },
      { label: 'Configurations…', run: () => (configOpen = true) },
      { label: 'Keys and mouse', run: () => openHelp('keys') },
      { label: 'Troubleshooting', run: () => openHelp('trouble') },
      { sep: true },
      { label: 'About Nebulosa Companion', run: () => openHelp('about') },
    ]);
  }

  // ---- the walkthrough: on the first run, and from Help ----------------------
  let touring = $state(false);

  function startTour() {
    helpOpen = false;
    // every part on show
    leftOpen = rightOpen = true;
    touring = true;
  }

  const TOUR: TourStep[] = [
    {
      title: 'Welcome to Nebulosa Companion',
      body: 'A quick look around: what each part of the window is for. <kbd>→</kbd> next, <kbd>←</kbd> back, <kbd>Esc</kbd> to leave. It is under <b>?</b> → Walkthrough any time.',
    },
    {
      target: 'sidebar',
      title: 'Connect a board',
      body: 'Boards on USB and Ethernet show here: click one. In the desktop app you can also type an IP address; recordings and logs you made are listed below.',
      when: () => !dev,
    },
    {
      target: 'device',
      title: 'The device',
      body: 'Its firmware, gateware and link. <b>Software update…</b> installs new firmware and gateware.',
    },
    {
      target: 'board',
      title: 'Power and the board',
      body: 'The measurement side (the analog front end) needs 12 V and must be <b>on</b>: switch it here. Also the input voltages, USB-C, the board clock (<b>Sync clock</b>) and the <b>Self-test</b>.',
    },
    {
      target: 'acquisition',
      title: 'Acquisition',
      body: 'The source (ADC for real measurements), the sample rate (decimation N = 1: 2 MS/s) and the current ranges. <b>Start</b> / <b>Stop</b> the stream.',
    },
    {
      target: 'modes',
      title: 'Scope or Logger',
      body: '<b>Scope</b>: a rolling oscilloscope, microseconds to seconds. <b>Logger</b>: the whole session, minutes to days, and logging to files.',
    },
    {
      target: 'strip',
      title: 'The scope’s controls',
      body: '<b>LIVE</b> / <b>HOLD</b> (<kbd>Space</kbd>), <b>Autoset</b>, the time per division, markers, and export: CSV, PNG, <b>Analyze</b>.',
    },
    {
      target: 'overview',
      title: 'The overview',
      body: 'Everything in the buffer; the box is what the plots show. Drag it to go back in time.',
    },
    {
      target: 'plots',
      title: 'The plots',
      body: 'Wheel: zoom in time. Drag across: a dual marker with Δt, mean and charge. Double-click: a single marker. <b>Right-click</b> for a menu: markers, trigger here, axis, export.',
    },
    {
      target: 'trigger',
      title: 'The trigger',
      body: '<b>Roll</b> like a chart recorder, or <b>Auto</b> / <b>Normal</b> / <b>Single</b> on an edge, a window, a pulse width or a timeout. Drag its level line on the plot.',
    },
    {
      target: 'meter',
      title: 'The meter',
      body: 'Mean and statistics of the screen, the run, or the selection (⚙ chooses which); charge and energy since you cleared them; and battery life for a capacity you enter.',
    },
    {
      target: 'lists',
      title: 'Markers and events',
      body: 'Every marker with its measurements; the trigger’s events with their width and charge. Click one to go to it; right-click for more.',
    },
    {
      target: 'config',
      title: 'Save your setup',
      body: 'The <b>⚙</b> button keeps <b>configurations</b>: save the current settings as a named profile, apply one, pick one to load on boot, or share a file and apply only part of it (just the trigger, say). Set up the bench once, then recall it with a click.',
    },
    {
      target: 'help',
      title: 'Help is here',
      body: 'Getting started, keys and mouse, troubleshooting, about, and this walkthrough again. <b>Learn</b>, next to it: hands-on tutorials (markers, triggers, events, the meter, math, analysis, the logger) and showcases of real measurements, on the demo device. <b>Aa</b> next to it makes the UI bigger or smaller; ◧ ◨ hide the side panels.',
    },
  ];

  // ---- the board (power, analog front end) ------------------------------------
  let board = $state<boardCmd.BoardStatus | null>(null);
  $effect(() => {
    if (!dev) board = null;
  });
  /** measuring real signals with the analog front end's supplies off */
  const frontEndOff = $derived(!!board && source === SRC.AD7387 && !(board.apwr_h && board.apwr_l));

  async function frontEndOn() {
    if (!dev) return;
    error = '';
    try {
      await boardCmd.set(dev, boardCmd.SET.APWR_H, 1);
      board = await boardCmd.set(dev, boardCmd.SET.APWR_L, 1);
    } catch (e) {
      error = `Analog front end: ${boardCmd.reason(e)}`;
    }
  }
  /** the board restarts (an update): connect to it again when it's back */
  let reconnectKey = '';
  async function boardRestarts() {
    reconnectKey = lastCandidate;
    note = 'The board is restarting…';
    await disconnect();
    for (let k = 0; k < 15; k++) {
      await new Promise((r) => setTimeout(r, 1000));
      await scan();
      const c = candidates.find((x) => x.key === reconnectKey);
      if (c) {
        await connect(c);
        if (dev) {
          note = `Reconnected: firmware ${info?.fw_version_str ?? ''}`;
          return;
        }
      }
    }
    note = 'The board hasn\'t come back yet: connect when it has.';
  }

  // ---- autoset ----------------------------------------------------------------

  /**
   * Like a scope's: the most active signal of the last seconds, an edge
   * trigger at 50 % of its swing, a time/div showing ~2.5 periods, Auto mode,
   * axes on auto. No periodic activity: Roll.
   */
  async function autoset() {
    if (store.empty) return;
    error = '';
    const t1 = store.tEnd;
    const t0 = Math.max(store.tStart, t1 - 4);
    // the levels from short averages (4000 columns over 4 s: ~1 ms each), so a
    // noise spike can't pull the trigger level away from the signal
    const L0 = await store.lod(t0, t1, 4000);
    const levels = (s: Signal) => {
      let lo = Infinity, hi = -Infinity, sum = 0, n = 0;
      for (const a of L0.ch[s].avg) {
        if (a == null) continue;
        if (a < lo) lo = a;
        if (a > hi) hi = a;
        sum += a;
        n++;
      }
      return { lo, hi, mean: n ? sum / n : 0, n };
    };
    // the most active: largest swing for its level (power follows current)
    const score = (s: Signal) => {
      const m = levels(s);
      return m.n ? (m.hi - m.lo) / Math.max(Math.abs(m.mean), (m.hi - m.lo) * 0.01, 1e-12) : 0;
    };
    const src: Signal = score('v') > 1.5 * score('i') ? 'v' : 'i';
    const m = levels(src);
    for (const s of SIGNALS) (axes[s].auto = true), (axes[s].log = false);
    view.live = true;
    if (!m.n || m.hi - m.lo < Math.max(Math.abs(m.mean) * 0.02, 1e-12)) {
      // flat: nothing to trigger on
      trig.mode = 'roll';
      view.divIndex = TIME_DIVS.indexOf(0.1);
      autosetMsg('No activity to trigger on: rolling at 100 ms/div');
      return;
    }
    const level = (m.lo + m.hi) / 2;
    const hyst = (m.hi - m.lo) * 0.1;

    /** median interval between rising crossings of the level over [a, b] (on short averages) */
    async function period(a: number, b: number): Promise<number | null> {
      const L = await store.lod(a, b, 8000);
      const c = L.ch[src];
      let armed = false;
      const ups: number[] = [];
      for (let k = 0; k < L.t.length; k++) {
        const v = c.avg[k];
        if (v == null) {
          armed = false; // a gap
          continue;
        }
        if (armed && v >= level) {
          ups.push(L.t[k]);
          armed = false;
        } else if (v < level - hyst) armed = true;
      }
      if (ups.length < 3) return null;
      const d = ups.slice(1).map((t, k) => t - ups[k]).sort((x, y) => x - y);
      return d[d.length >> 1];
    }

    let p = await period(t0, t1);
    // short next to the resolution (4 s / 8000 = 0.5 ms): zoom in and measure again
    if (p != null && p < ((t1 - t0) / 8000) * 40) p = (await period(Math.max(store.tStart, t1 - p * 60), t1)) ?? p;
    if (p == null) {
      // one-off or slow: trigger anyway, show a few seconds
      p = 4;
    }
    let k = TIME_DIVS.findIndex((d) => d * 10 >= p! * 2.5);
    if (k < 0) k = TIME_DIVS.length - 1;
    view.divIndex = k;
    trig.source = src;
    trig.kind = 'edge';
    trig.slope = 'rise';
    trig.level = level;
    trig.hysteresis = hyst;
    trig.holdoff = 0;
    trig.filter = 0;
    trig.position = 0.3;
    trig.mode = 'auto';
    view.live = true;
    autosetMsg(`${SIGNAL_INFO[src].name} ↑ at ${fmtSI(level, SIGNAL_INFO[src].unit, 3)} · period ${p >= 4 ? '–' : fmtTime(p)} · ${fmtTime(TIME_DIVS[k])}/div`);
  }

  /** what Autoset did, next to its button for a few seconds */
  let autosetNote = $state('');
  let autosetTimer: ReturnType<typeof setTimeout> | undefined;
  function autosetMsg(m: string) {
    autosetNote = m;
    clearTimeout(autosetTimer);
    autosetTimer = setTimeout(() => (autosetNote = ''), 6000);
  }

  /** the plots as they first were: equal heights, axes on auto, mean and min–max, 100 ms/div, live */
  function resetView() {
    trace = { mean: true, range: true, sigma: false };
    for (const s of SIGNALS) (axes[s].auto = true), (axes[s].log = false);
    scope?.resetLayout();
    logger?.resetLayout();
    if (appMode === 'logger') lv = { follow: true, last: 0, t0: 0, t1: 0 };
    else {
      view.divIndex = TIME_DIVS.indexOf(0.1);
      view.live = true;
    }
  }

  // ---- scope / logger ------------------------------------------------------------
  let appMode = $state<'scope' | 'logger' | 'sca'>(stored('nebulosa.modeIsLogger', 0) === 1 ? 'logger' : 'scope');
  function setAppMode(m: 'scope' | 'logger' | 'sca') {
    appMode = m;
    save('nebulosa.modeIsLogger', m === 'logger' ? 1 : 0);
  }

  // the side-channel tab needs the demo playing its target (a leaky MCU); any
  // other tab goes back to the sensor load. A showcase sets its own scenario.
  let sca = $state<SideChannel>();
  /** wait for the Side-channel component to mount (the tab just opened) */
  async function whenSca(): Promise<boolean> {
    for (let k = 0; k < 40 && !sca; k++) await new Promise((r) => setTimeout(r, 25));
    return !!sca;
  }
  let scaScenarioSet = false;
  $effect(() => {
    const d = dev?.link;
    if (!(d instanceof DemoLink)) return;
    if (appMode === 'sca' && !showcase) {
      d.setScenario('target');
      scaScenarioSet = true;
    } else if (scaScenarioSet && d.scenario === 'target' && !showcase) {
      d.setScenario('iot');
      scaScenarioSet = false;
    }
  });

  let lv = $state<LogView>({ follow: true, last: 0, t0: 0, t1: 0 });
  /** the session log's state, reactive (the store's copy isn't) */
  let sess = $state.raw<Meta['session']>({ has_data: false, t_start: 0, t_end: 0, interval: 0.1, entries: 0, version: 0, bytes: 0 });
  /** unix time − FPGA time, s: wall clock for the logger */
  let wallOffset = $state(Date.now() / 1000);
  let wallPending: number | null = null; // a file's start time (unix s), set at its first Meta

  /** the logger's span now, FPGA time */
  function loggerSpan(): [number, number] {
    const s = store.session;
    if (!lv.follow) return [lv.t0, lv.t1];
    return [lv.last > 0 ? Math.max(s.t_start, s.t_end - lv.last) : s.t_start, s.t_end];
  }

  // ---- moving around: history, the Logger's keys, stepping through things ----------

  /** the Scope's view, as the history keeps it */
  type ScopeNav = { live: boolean; end: number; div: number };
  const scopeHist = new ViewHistory<ScopeNav>(
    (a, b) => a.live === b.live && a.div === b.div && (a.live || Math.abs(a.end - b.end) <= 1e-9 * Math.max(1, Math.abs(a.end))),
    (s) => {
      view.divIndex = s.div;
      if (s.live) view.live = true;
      else {
        view.live = false;
        view.end = s.end;
      }
    },
  );
  $effect(() => {
    const s: ScopeNav = { live: view.live, end: view.live ? 0 : view.end, div: view.divIndex };
    if (hasData) scopeHist.observe(s);
  });
  const loggerHist = new ViewHistory<LogView>(
    (a, b) => a.follow === b.follow && a.last === b.last && (a.follow || (a.t0 === b.t0 && a.t1 === b.t1)),
    (s) => (lv = { ...s }),
  );
  $effect(() => {
    const s = { ...lv };
    loggerHist.observe(s);
  });
  const hist = $derived(appMode === 'logger' ? loggerHist : scopeHist);
  function navBack() {
    if (appMode !== 'sca' && !hist.back()) toast('Nothing to go back to');
  }
  function navForward() {
    if (appMode !== 'sca') hist.forward();
  }

  /** the Logger at [t0, t1], not following */
  function loggerSet(t0: number, t1: number) {
    lv = { follow: false, last: lv.last, t0, t1 };
  }
  function loggerPan(frac: number) {
    const [a, b] = loggerSpan();
    const d = (b - a) * frac;
    loggerSet(a + d, b + d);
  }
  function loggerZoom(f: number) {
    const [a, b] = loggerSpan();
    const c = (a + b) / 2;
    const w = Math.max((b - a) * f, 2e-5);
    loggerSet(c - w / 2, c + w / 2);
  }
  /** following the newest entries, or staying where the view is */
  function toggleFollow() {
    if (lv.follow) loggerSet(...loggerSpan());
    else lv = { ...lv, follow: true };
  }
  /** the start of the session, at the span shown */
  function loggerHome() {
    const [a, b] = loggerSpan();
    const s0 = store.session.t_start;
    loggerSet(s0, s0 + (b - a));
  }

  /** the Scope at the oldest samples the buffer holds */
  function scopeHome() {
    if (store.empty) return;
    view.live = false;
    view.end = store.tStart + view.windowS;
  }

  /** where "next" counts from in the Scope: the trigger position on screen */
  function scopeRef() {
    const [a, b] = view.span(store);
    return b - (1 - trig.position) * (b - a);
  }

  /** the next (dir 1) or previous (-1) of `ts` (sorted) after / before `ref`, or null */
  function stepIn(ts: (k: number) => number, n: number, ref: number, dir: number, eps: number): number | null {
    let lo = 0, hi = n;
    while (lo < hi) {
      const m = (lo + hi) >> 1;
      if (ts(m) <= ref + (dir > 0 ? eps : -eps)) lo = m + 1;
      else hi = m;
    }
    const k = dir > 0 ? lo : lo - 1;
    return k >= 0 && k < n ? k : null;
  }

  /** N / Shift+N: the next / previous trigger event */
  function stepEvent(dir: number) {
    const ev = trig.events;
    if (!ev.length) return toast(trig.detecting ? 'No events yet' : 'No events: choose a trigger mode, or turn on Log events in Roll');
    const k = stepIn((i) => ev[i].t, ev.length, scopeRef(), dir, view.windowS * 1e-4);
    if (k == null) return toast(dir > 0 ? 'No later event' : 'No earlier event');
    jumpToEvent(ev[k]);
  }

  /** , / . : the previous / next serial message */
  function stepSerial(dir: number) {
    const ms = monitor.annotations;
    if (!ms.length) return toast('No serial messages');
    if (appMode === 'logger') {
      const [a, b] = loggerSpan();
      const k = stepIn((i) => ms[i].t, ms.length, (a + b) / 2, dir, (b - a) * 1e-4);
      if (k == null) return toast(dir > 0 ? 'No later message' : 'No earlier message');
      jumpLoggerTo(ms[k].t);
    } else {
      const k = stepIn((i) => ms[i].t, ms.length, scopeRef(), dir, view.windowS * 1e-4);
      if (k == null) return toast(dir > 0 ? 'No later message' : 'No earlier message');
      jumpToSerial(ms[k].t);
    }
  }

  /** < / > : the previous / next marker, centred (the zoom kept) */
  function stepMarker(dir: number) {
    const ms = [...view.markers].sort((a, b) => a.t0 - b.t0);
    if (!ms.length) return toast('No markers: double-click a plot, or drag across one');
    const cur = ms.findIndex((m) => m.id === view.active);
    const k = cur < 0 ? (dir > 0 ? 0 : ms.length - 1) : (cur + dir + ms.length) % ms.length;
    const m = ms[k];
    view.active = m.id;
    view.hold(store);
    const mid = m.t1 == null ? m.t0 : (m.t0 + m.t1) / 2;
    view.end = mid + view.windowS / 2;
  }

  /** a moment of the Scope in the Logger: the long view around it */
  function showInLogger(t: number) {
    if (!store.session.has_data) return toast('The session log is empty');
    setAppMode('logger');
    const s0 = store.session;
    const cur = lv.follow ? (lv.last > 0 ? lv.last : s0.t_end - s0.t_start) : lv.t1 - lv.t0;
    const w = Math.min(Math.max(cur, 10), 600);
    loggerSet(t - w / 2, t + w / 2);
  }

  /** a moment of the logger in the scope: its raw samples, if still in memory */
  function openInScope(t: number) {
    if (store.empty || t < store.tStart || t > store.tEnd) {
      note = `${new Date((t + wallOffset) * 1000).toLocaleString()}: its samples are no longer in memory (the scope keeps the last ${fmtTime(store.tEnd - store.tStart)}). Record raw samples to keep them.`;
      return;
    }
    setAppMode('scope');
    view.hold(store);
    view.end = t + view.windowS / 2;
  }

  const SOURCE_NAME: Record<number, string> = { [SRC.STUB_RANDOM]: 'Stub: random', [SRC.STUB_PATTERN]: 'Stub: pattern', [SRC.AD7387]: 'ADC' };
  const LOG_INTERVALS = [0.001, 0.01, 0.1, 1, 10];
  const SCOPE_SECTIONS = [
    { id: 'device', icon: 'device', title: 'Device' },
    { id: 'board', icon: 'board', title: 'Board' },
    { id: 'acquisition', icon: 'acquisition', title: 'Acquisition' },
    { id: 'trigger', icon: 'trigger', title: 'Trigger' },
    { id: 'display', icon: 'display', title: 'Display' },
    { id: 'math', icon: 'math', title: 'Math channel' },
    { id: 'calibration', icon: 'calibration', title: 'Calibration' },
    { id: 'monitor', icon: 'monitor', title: 'Serial monitor' },
  ];
  const LOGGER_SECTIONS = [
    { id: 'logger', icon: 'logger', title: 'Logger' },
    { id: 'output', icon: 'output', title: 'File output' },
  ];
  // only the sections actually in the sidebar for this mode (the logger ones show in Logger)
  const navSections = $derived(appMode === 'logger' ? [...SCOPE_SECTIONS, ...LOGGER_SECTIONS] : SCOPE_SECTIONS);

  // ---- saved configurations (profiles) -----------------------------------
  let configOpen = $state(false);
  const SECTION_KEYS = ['device', 'board', 'acquisition', 'trigger', 'display', 'math', 'calibration', 'logger', 'monitor', 'output'];

  function sectionOpenStates(): Record<string, boolean> {
    const o: Record<string, boolean> = {};
    for (const id of SECTION_KEYS) {
      const v = localStorage.getItem(`nebulosa.section.${id}`);
      if (v === '0' || v === '1') o[id] = v === '1';
    }
    return o;
  }

  /** the current settings as a named config */
  function gatherConfig(name: string): Config {
    return {
      v: CONFIG_VERSION,
      name,
      saved: Date.now(),
      sections: {
        view: { appMode, meterScope, divIndex: view.divIndex, live: view.live },
        display: {
          trace: { ...trace },
          shown: { ...shown },
          axes: Object.fromEntries(SIGNALS.map((s) => [s, { auto: axes[s].auto, log: axes[s].log }])),
        },
        acquisition: { source, log2n, decimMode, rangeAuto, rangeManual, rangeMin, rangeMax, bufferMB },
        trigger: trig.dump(),
        logger: { logInterval, out: $state.snapshot(out) },
        serial: $state.snapshot(serialCfg),
        sidechannel: JSON.parse(localStorage.getItem('nebulosa.sca') ?? 'null') ?? undefined,
        layout: { leftW, rightW, listH, uiScale: uiScale.value, theme: uiTheme.value, sections: sectionOpenStates() },
      },
    };
  }

  /** apply a config, all of it or only the picked sections */
  function applyConfig(cfg: Config, pick?: Set<SectionId>) {
    const sec = cfg.sections as Record<SectionId, Record<string, unknown>>;
    const want = (id: SectionId) => sec[id] != null && (!pick || pick.has(id));
    if (want('view')) {
      const v = sec.view;
      if (typeof v.appMode === 'string') setAppMode(v.appMode as typeof appMode);
      if (typeof v.meterScope === 'string') meterScope = v.meterScope as MeterScope;
      if (typeof v.divIndex === 'number') view.divIndex = v.divIndex;
      view.live = !!v.live;
    }
    if (want('display')) {
      const v = sec.display;
      if (v.trace) Object.assign(trace, v.trace);
      if (v.shown) Object.assign(shown, v.shown);
      if (v.axes) for (const si of SIGNALS) { const a = (v.axes as Record<string, { auto: boolean; log: boolean }>)[si]; if (a) { axes[si].auto = !!a.auto; axes[si].log = !!a.log; } }
    }
    if (want('acquisition')) {
      const v = sec.acquisition;
      if (typeof v.source === 'number') source = v.source;
      if (typeof v.log2n === 'number') log2n = v.log2n;
      if (typeof v.decimMode === 'number') decimMode = v.decimMode;
      if (typeof v.rangeAuto === 'boolean') rangeAuto = v.rangeAuto;
      if (typeof v.rangeManual === 'number') rangeManual = v.rangeManual;
      if (typeof v.rangeMin === 'number') rangeMin = v.rangeMin;
      if (typeof v.rangeMax === 'number') rangeMax = v.rangeMax;
      if (typeof v.bufferMB === 'number' && v.bufferMB !== bufferMB) { bufferMB = v.bufferMB; if (dev) applyBuffer(); }
    }
    if (want('trigger')) trig.restore(sec.trigger);
    if (want('logger')) {
      const v = sec.logger;
      if (typeof v.logInterval === 'number' && LOG_INTERVALS.includes(v.logInterval)) logInterval = v.logInterval;
      if (v.out) out = v.out as OutputCfg;
    }
    if (want('serial')) Object.assign(serialCfg, sec.serial);
    if (want('sidechannel')) { try { localStorage.setItem('nebulosa.sca', JSON.stringify(sec.sidechannel)); } catch { /* ignore */ } }
    if (want('layout')) {
      const v = sec.layout;
      if (typeof v.leftW === 'number') { leftW = v.leftW; save('nebulosa.leftW', leftW); }
      if (typeof v.rightW === 'number') { rightW = v.rightW; save('nebulosa.rightW', rightW); }
      if (typeof v.listH === 'number') listH = v.listH;
      if (typeof v.uiScale === 'number' && canScale) setScale(v.uiScale);
      if (typeof v.theme === 'string') setTheme(v.theme as 'system' | 'dark' | 'light');
      const so = v.sections as Record<string, boolean> | undefined;
      if (so) for (const [id, open] of Object.entries(so)) {
        try { localStorage.setItem(`nebulosa.section.${id}`, open ? '1' : '0'); } catch { /* ignore */ }
        if (open) dispatchEvent(new CustomEvent('nebulosa:open-section', { detail: id }));
      }
    }
  }
  // the serial monitor: its messages become markers on the logger, and (when
  // logging to a file) a sidecar <log>.serial.csv next to it
  const BAUDS = [300, 1200, 2400, 4800, 9600, 14400, 19200, 28800, 38400, 57600, 76800, 115200, 230400, 250000, 460800, 500000, 921600, 1000000, 1500000, 2000000, 3000000];
  let baudCustom = $state(false);
  let serialCfg = $state<SerialConfig>(loadSerialCfg());
  function loadSerialCfg(): SerialConfig {
    try {
      const v = JSON.parse(localStorage.getItem('nebulosa.serial') ?? 'null');
      if (v && typeof v.baud === 'number') return { ...defaultSerialConfig, ...v, trigger: { ...defaultSerialConfig.trigger, ...v.trigger } };
    } catch {
      /* storage unavailable */
    }
    return { ...defaultSerialConfig };
  }
  $effect(() => save('nebulosa.serial', $state.snapshot(serialCfg)));
  const monitor = new SerialMonitor(
    () => store.tEnd,
    () => serialCfg,
    (a) => {
      // a trigger match freezes the view on it, if asked: in the Scope like a
      // trigger (the match at the trigger position), in the Logger around it
      if (!serialCfg.trigger.hold) return;
      if (appMode === 'scope') {
        view.hold(store);
        view.end = a.t + (1 - trig.position) * view.windowS;
      } else if (appMode === 'logger') {
        const w = lv.follow ? (lv.last > 0 ? lv.last : 60) : lv.t1 - lv.t0;
        lv = { follow: false, last: lv.last, t0: a.t - w / 2, t1: a.t + w / 2 };
      }
    },
  );
  /** messages captured since this log started, for the sidecar at stop */
  let monitorLogStart = 0;
  let sendText = $state('');
  let monitorHex = $state(false);
  let sendHistory = $state<string[]>([]);
  let histIdx = -1;
  async function monitorSend() {
    if (!sendText) return;
    await monitor.send(sendText);
    sendHistory = [...sendHistory.filter((h) => h !== sendText), sendText].slice(-50);
    histIdx = -1;
    sendText = '';
  }
  function sendKey(e: KeyboardEvent) {
    if (e.key === 'Enter') return monitorSend();
    if (e.key === 'ArrowUp' && sendHistory.length) {
      e.preventDefault();
      histIdx = histIdx < 0 ? sendHistory.length - 1 : Math.max(0, histIdx - 1);
      sendText = sendHistory[histIdx];
    } else if (e.key === 'ArrowDown' && histIdx >= 0) {
      e.preventDefault();
      histIdx = histIdx + 1 >= sendHistory.length ? -1 : histIdx + 1;
      sendText = histIdx < 0 ? '' : sendHistory[histIdx];
    }
  }
  /** each message's text as hex of its bytes (the hex view) */
  function toHexLine(t: string) {
    return [...new TextEncoder().encode(t)].map((b) => b.toString(16).padStart(2, '0')).join(' ');
  }
  /** save the captured messages now (independent of a file log) */
  async function saveMessages() {
    const msgs = monitor.annotations;
    if (!msgs.length) return;
    const head = 'unix_time_s,fpga_time_s,wall_clock,kind,message\n';
    const body = msgs.map((a) => `${((a.wall ?? 0) / 1000).toFixed(3)},${a.t.toFixed(6)},${new Date(a.wall ?? 0).toISOString()},${a.kind ?? 'rx'},${csvField(a.text)}`).join('\n');
    const bytes = new TextEncoder().encode(head + body + '\n');
    const name = `nebulosa-serial-${stamp()}.csv`;
    try {
      if (isNative) {
        const path = await native.pickSave(`${recDir || '.'}/${name}`, 'CSV', 'csv');
        if (!path) return;
        await native.saveBytes(path, bytes);
        note = `Saved ${msgs.length} messages`;
      } else {
        const a = document.createElement('a');
        a.href = URL.createObjectURL(new Blob([bytes], { type: 'text/csv' }));
        a.download = name;
        a.click();
        setTimeout(() => URL.revokeObjectURL(a.href), 1000);
      }
    } catch (e) {
      note = String(e);
    }
  }
  // a synthetic serial feed for the simultaneous-serial showcase (no real port)
  let demoFeed: ReturnType<typeof setInterval> | undefined;
  function startDemoFeed() {
    stopDemoFeed();
    if (!monitor.open) monitor.note('--- serial monitor (demo) ---');
    let n = 0;
    const lines = [
      () => 'boot: firmware 1.4.2',
      () => `wake, reading sensors`,
      () => `temp=${(22 + Math.random() * 3).toFixed(1)}C  rh=${(40 + Math.random() * 10).toFixed(0)}%`,
      () => `tx packet #${n} (${12 + Math.floor(Math.random() * 6)} bytes)`,
      () => 'ack, sleeping',
    ];
    demoFeed = setInterval(() => {
      monitor.note(lines[n % lines.length]());
      n++;
    }, 900);
  }
  function stopDemoFeed() {
    clearInterval(demoFeed);
    demoFeed = undefined;
  }
  let monitorPort = $state('');
  let monitorPorts = $state<{ path: string; label: string }[]>([]);
  function csvField(s: string) {
    return /[",\n]/.test(s) ? '"' + s.replace(/"/g, '""') + '"' : s;
  }
  /** the serial messages captured during this log, as a sidecar next to it */
  async function writeSerialSidecar(logFile?: string) {
    const msgs = monitor.since(monitorLogStart);
    if (!isNative || !logFile || !msgs.length) return;
    const head = 'unix_time_s,fpga_time_s,wall_clock,kind,message\n';
    const body = msgs
      .map((a) => `${((a.wall ?? 0) / 1000).toFixed(3)},${a.t.toFixed(6)},${new Date(a.wall ?? 0).toISOString()},${a.kind ?? 'rx'},${csvField(a.text)}`)
      .join('\n');
    const path = logFile.replace(/\.[^.]+$/, '') + '.serial.csv';
    try {
      await native.saveBytes(path, new TextEncoder().encode(head + body + '\n'));
    } catch (e) {
      note = `Serial sidecar not saved: ${e}`;
    }
  }
  async function monitorConnect() {
    if (monitor.open) return monitor.disconnect();
    if (isNative && !monitorPorts.length) await scanMonitorPorts();
    await monitor.connect(isNative ? monitorPort : undefined);
  }
  async function scanMonitorPorts() {
    try {
      const { nativeSerialPorts } = await import('./lib/targets');
      monitorPorts = await nativeSerialPorts();
      if (!monitorPort && monitorPorts[0]) monitorPort = monitorPorts[0].path;
    } catch (e) {
      note = String(e);
    }
  }
  let logInterval = $state(stored('nebulosa.logInterval', 0.1, (v) => LOG_INTERVALS.includes(v)));
  let logging = $state<Meta['logging'] | null>(null);
  let logPath = $state('');
  let exporting = $state(false);
  let exportBytes = $state(0);
  let logStarted = $state(0);
  let nowTick = $state(Date.now());
  /** the logger's output (format, data, fields): kept across sessions */
  let out = $state<OutputCfg>(loadOut());
  function loadOut(): OutputCfg {
    try {
      const v = JSON.parse(localStorage.getItem('nebulosa.output') ?? 'null');
      if (v && v.kind && v.cfg?.fields) return v;
    } catch {
      /* storage unavailable */
    }
    return defaultOutput();
  }
  $effect(() => save('nebulosa.output', $state.snapshot(out)));
  let rawCapGB = $state(stored('nebulosa.rawCapGB', 20));
  let rawByLogger = false;
  let diskFree = $state<number | null>(null);

  async function changeLogInterval(iv: number, sel?: HTMLSelectElement) {
    if (store.session.has_data && !(await ask('A new interval starts the session log over. Go on?'))) {
      if (sel) sel.value = String(logInterval); // cancelled: show the interval still in use
      return;
    }
    logInterval = iv;
    save('nebulosa.logInterval', iv);
    await activeLink?.setLogInterval(iv);
  }

  async function newSession() {
    if (!(await ask('Clear the session log and start over?'))) return;
    await activeLink?.clearSession();
    lv = { follow: true, last: 0, t0: 0, t1: 0 };
  }

  /** what the log holds: interval statistics, or every sample as measured */
  function setLogData(d: 'summary' | 'raw') {
    out.cfg.data = d;
    if (d !== 'raw') return;
    // samples are 0.5 µs apart at 2 MS/s: time needs 0.1 µs; counts are per interval
    for (const f of out.cfg.fields) {
      if (f.what === 'time' && out.cfg.time !== 'iso' && f.scale === 1) f.decimals = Math.max(f.decimals, 7);
      if (f.what === 'count') f.what = 'range';
    }
  }

  /** rough size of an every-sample log: 4 bytes a sample natively, ~11 characters a column as text */
  const rawBytesPerS = $derived(rateHz * (out.kind === 'native' ? 4 : out.kind === 'hdf5' ? 4 * out.cfg.fields.length : 11 * out.cfg.fields.length));

  async function startLog() {
    if (out.kind !== 'native') return startExportLog();
    error = '';
    try {
      save('nebulosa.rawCapGB', rawCapGB);
      monitorLogStart = monitor.total;
      logPath = await native.startLogging({
        app: 'Nebulosa Companion',
        created: new Date().toISOString(),
        wall_offset: wallOffset,
        interval: logInterval,
        link: dev?.link.description,
        device: info,
        rate_hz: rateHz,
        decimation: { log2n, mode: decimMode },
        calibration: $state.snapshot(cal),
        math: mathInfo.on ? $state.snapshot(mathInfo) : null,
      });
      note = `Logging to ${logPath}`;
      logStarted = Date.now();
      if (out.cfg.data === 'raw' && !recording?.active) {
        await toggleRecording();
        rawByLogger = true;
      }
    } catch (e) {
      error = String(e);
    }
  }

  async function startExportLog() {
    error = '';
    try {
      logPath = await native.startExport($state.snapshot(out.cfg), wallOffset, logHeader());
      exporting = true;
      exportBytes = 0;
      logStarted = Date.now();
      note = `Logging to ${logPath}`;
    } catch (e) {
      error = String(e);
    }
  }

  function logHeader() {
    return {
      app: 'Nebulosa Companion',
      created: new Date().toISOString(),
      wall_offset: wallOffset,
      interval: logInterval,
      link: dev?.link.description,
      device: info,
      rate_hz: rateHz,
      decimation: { log2n, mode: decimMode },
      calibration: $state.snapshot(cal),
      math: mathInfo.on ? $state.snapshot(mathInfo) : null,
    };
  }

  async function stopLog(reason?: string) {
    if (exporting) {
      try {
        const r = await native.stopExport();
        if (r) note = `${reason ? reason + ' ' : ''}Saved ${r.path.split(/[\\/]/).pop()} · ${r.rows.toLocaleString()} rows · ${fmtBytesSize(r.bytes)}`;
      } catch (e) {
        error = String(e);
      } finally {
        exporting = false;
        logStarted = 0;
      }
      return;
    }
    logStarted = 0;
    try {
      const r = await native.stopLogging();
      if (rawByLogger && recording?.active) await toggleRecording();
      rawByLogger = false;
      await writeSerialSidecar(r?.path);
      if (r) note = `${reason ? reason + ' ' : ''}Log saved: ${r.path.split(/[\\/]/).pop()} · ${r.entries.toLocaleString()} entries · ${fmtBytesSize(r.bytes)}`;
      refreshRecordings();
    } catch (e) {
      error = String(e);
    }
  }

  /** every 2 s while writing: stop cleanly on a write error, a low disk, or the raw limit */
  async function watchdog() {
    nowTick = Date.now();
    if (!isNative || !(logging?.active || recording?.active || exporting)) return;
    try {
      const d = await native.diskStatus();
      diskFree = d.free;
      exportBytes = d.export_bytes;
      if (exporting && d.export_error) {
        error = `Logging stopped: ${d.export_error}`;
        await stopLog();
        return;
      }
      if (exporting && d.free < 3e8) {
        await stopLog('Disk nearly full:');
        return;
      }
      if (d.logging_error) {
        error = `Logging stopped: ${d.logging_error}`;
        await stopLog();
      } else if (d.recording_error) {
        error = `Raw recording stopped: ${d.recording_error}`;
        await toggleRecording();
      } else if (recording?.active && d.free < 2e9) {
        await toggleRecording();
        note = 'Raw recording stopped: less than 2 GB free.';
      } else if (logging?.active && d.free < 3e8) {
        await stopLog('Disk nearly full:');
      } else if (recording?.active && recording.bytes > rawCapGB * 1e9) {
        await toggleRecording();
        note = `Raw recording stopped at its ${rawCapGB} GB limit (the log goes on).`;
      } else if (d.dropped > 0) {
        error = `The disk isn't keeping up: ${d.dropped} chunks lost.`;
      }
    } catch (e) {
      console.warn('watchdog', e);
    }
  }

  // ---- analysis ---------------------------------------------------------------
  type AnalysisTab = 'histogram' | 'cdf' | 'psd' | 'maxwindow';
  let analysis = $state<{ span: [number, number]; label: string; tab?: AnalysisTab; autorun?: boolean } | null>(null);

  function openAnalysis() {
    const m = view.activeMarker;
    if (m && m.t1 != null) analysis = { span: [m.t0, m.t1], label: `${markerName(m)}: ${fmtTime(m.t1 - m.t0)}` };
    else {
      const sp = view.span(store);
      analysis = { span: sp, label: `the screen: ${fmtTime(sp[1] - sp[0])}` };
    }
  }

  // ---- showcases: the demo device plays a real load, a guide walks through it ----------
  let showcasesOpen = $state(false);

  // ---- Ctrl+K: find a setting or an action -------------------------------------
  let paletteOpen = $state(false);

  /** goes to a settings section: the view it is in, the panel shown, the section open and in view */
  function gotoSection(id: string) {
    if (!info) return toast('Connect a device first (or try the demo device)');
    if (LOGGER_SECTIONS.some((x) => x.id === id) && appMode !== 'logger') setAppMode('logger');
    else if (appMode === 'sca') setAppMode('scope');
    if (!leftOpen) toggleSide('left');
    requestAnimationFrame(() => {
      dispatchEvent(new CustomEvent('nebulosa:open-section', { detail: id }));
      requestAnimationFrame(() => {
        const el = document.querySelector(`[data-sec="${id}"]`);
        el?.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
        el?.classList.add('flash');
        setTimeout(() => el?.classList.remove('flash'), 700);
      });
    });
  }

  /** everything the search finds: actions, views, the settings sections and each option in them */
  function commands(): Command[] {
    const live = appMode !== 'sca';
    const c: Command[] = [
      { group: 'View', label: 'Scope', hint: 'Ctrl 1', run: () => setAppMode('scope') },
      { group: 'View', label: 'Logger', hint: 'Ctrl 2', keywords: 'long term session', run: () => setAppMode('logger') },
      { group: 'View', label: 'Side-channel', hint: 'Ctrl 3', keywords: 'sca cpa aes power analysis', run: () => setAppMode('sca') },
      { group: 'Action', label: streaming ? 'Stop the stream' : 'Start the stream', disabled: !dev, keywords: 'run acquisition', run: () => (streaming ? stop() : start()) },
      { group: 'Action', label: appMode === 'logger' ? (lv.follow ? 'Hold (stay here)' : 'Live (follow the newest)') : view.live ? 'Hold' : 'Live', hint: 'Space', disabled: !live, keywords: 'follow freeze pause', run: () => (appMode === 'logger' ? toggleFollow() : toggleLive()) },
      { group: 'Action', label: 'Back to the view before', hint: 'Alt ←', disabled: !live, keywords: 'history previous undo zoom', run: navBack },
      { group: 'Action', label: 'Forward', hint: 'Alt →', disabled: !live, keywords: 'history next', run: navForward },
      { group: 'Action', label: 'Reset view', keywords: 'zoom axes default', run: resetView },
      { group: 'Action', label: 'Show everything', keywords: 'fit all zoom out whole', disabled: !hasData, run: () => (appMode === 'logger' ? (lv = { follow: true, last: 0, t0: 0, t1: 0 }) : fitAll()) },
      { group: 'Action', label: 'Go to the oldest data', hint: 'Home', disabled: !live || !hasData, keywords: 'start beginning', run: () => (appMode === 'logger' ? loggerHome() : scopeHome()) },
      { group: 'Action', label: 'Go to the newest data', hint: 'End', disabled: !live, keywords: 'now live end', run: () => (appMode === 'logger' ? (lv = { ...lv, follow: true }) : (view.live = true)) },
      { group: 'Action', label: 'Autoset', disabled: !hasData, keywords: 'automatic scale trigger', run: () => (setAppMode('scope'), autoset()) },
      { group: 'Action', label: 'Add a marker', hint: 'M', disabled: !hasData, run: () => (setAppMode('scope'), addSingleMarker()) },
      { group: 'Action', label: 'Add a dual marker', hint: 'D', disabled: !hasData, keywords: 'window measure delta', run: () => (setAppMode('scope'), addDualMarker()) },
      { group: 'Action', label: 'Next marker', hint: '>', disabled: !view.markers.length, run: () => (setAppMode('scope'), stepMarker(1)) },
      { group: 'Action', label: 'Previous marker', hint: '<', disabled: !view.markers.length, run: () => (setAppMode('scope'), stepMarker(-1)) },
      { group: 'Action', label: 'Clear the markers', disabled: !view.markers.length, run: clearMarkersUndoable },
      { group: 'Action', label: 'Next trigger event', hint: 'N', keywords: 'event trigger step', run: () => (setAppMode('scope'), stepEvent(1)) },
      { group: 'Action', label: 'Previous trigger event', hint: 'Shift N', keywords: 'event trigger step', run: () => (setAppMode('scope'), stepEvent(-1)) },
      { group: 'Action', label: 'Next serial message', hint: '.', keywords: 'uart message step', run: () => stepSerial(1) },
      { group: 'Action', label: 'Previous serial message', hint: ',', keywords: 'uart message step', run: () => stepSerial(-1) },
      { group: 'Action', label: 'Show this moment in the Logger', hint: 'L', disabled: !store.session.has_data, keywords: 'long term context', run: () => showInLogger(scopeRef()) },
      { group: 'Action', label: 'Analyze…', hint: 'A', disabled: !hasData, keywords: 'histogram cdf spectrum fft worst window', run: openAnalysis },
      { group: 'Action', label: 'Export samples as CSV…', disabled: !hasData, keywords: 'save csv export', run: exportCsv },
      { group: 'Action', label: 'Save the plots as PNG…', disabled: !hasData, keywords: 'image screenshot picture', run: exportPng },
      { group: 'Action', label: 'Set the trigger by clicking a plot', disabled: !hasData, keywords: 'trigger level pick', run: () => (setAppMode('scope'), (pickTrigger = true)) },
      { group: 'Action', label: 'Start logging to file', disabled: !isNative || !dev || !!logging?.active || exporting, keywords: 'record save log file every sample', run: () => (setAppMode('logger'), startLog()) },
      { group: 'Action', label: 'Hide / show the settings panel', hint: '[', keywords: 'sidebar left', run: () => toggleSide('left') },
      { group: 'Action', label: 'Hide / show the meter', hint: ']', keywords: 'sidebar right panel', run: () => toggleSide('right') },
      { group: 'Action', label: 'Configurations…', keywords: 'profiles save load settings', run: () => (configOpen = true) },
      { group: 'Action', label: 'Theme: dark', keywords: 'appearance colours', run: () => setTheme('dark') },
      { group: 'Action', label: 'Theme: light', keywords: 'appearance colours', run: () => setTheme('light') },
      { group: 'Action', label: 'Try the demo device', disabled: !!dev, keywords: 'connect synthetic', run: startDemo },
      { group: 'Action', label: 'Disconnect', disabled: !dev, run: disconnect },
      { group: 'Help', label: 'Help', hint: 'F1', keywords: 'keys manual', run: () => openHelp('start') },
      { group: 'Help', label: 'Keys and mouse', keywords: 'shortcuts keyboard', run: () => openHelp('keys') },
      { group: 'Help', label: 'Walkthrough', keywords: 'tour', run: startTour },
      { group: 'Help', label: 'Tutorials and showcases', keywords: 'learn examples', run: () => (showcasesOpen = true) },
    ];
    for (const sec of [...SCOPE_SECTIONS, ...LOGGER_SECTIONS]) {
      c.push({ group: 'Settings', label: sec.title, detail: SECTION_HELP[sec.id]?.intro, run: () => gotoSection(sec.id) });
      // each option of the section (as its help lists them): found by its name and what it does
      for (const it of SECTION_HELP[sec.id]?.items ?? [])
        c.push({ group: 'Option', label: it.name, detail: sec.title, keywords: `${it.text} ${(it.list ?? []).flat().join(' ')}`, run: () => gotoSection(sec.id) });
    }
    return c;
  }
  let showcase = $state.raw<{ s: Showcase; steps: TourStep[] } | null>(null);
  /** the newest sample when the showcase's load started: data before it is another load */
  let scenarioSince = 0;

  const sleepMs = (ms: number) => new Promise((r) => setTimeout(r, ms));

  const showcaseCtx: ShowcaseCtx = {
    mode: (m) => setAppMode(m),
    live(div) {
      const k = TIME_DIVS.indexOf(div);
      if (k >= 0) view.divIndex = k;
      view.live = true;
    },
    trigger: (t) => void Object.assign(trig, t),
    async math(m) {
      if (!m) return mathInfo.on ? mathOff() : undefined;
      mathDraft = { ...m };
      await applyMath();
    },
    logAxis(sig, on) {
      axes[sig].auto = true;
      axes[sig].log = on;
    },
    async newest(atLeast = 0) {
      for (let k = 0; k < 100 && (store.empty || store.tEnd - Math.max(store.tStart, scenarioSince) < atLeast); k++) await sleepMs(50);
      return store.tEnd;
    },
    async triggered() {
      for (let k = 0; k < 160; k++) {
        if (trig.shown != null && (trig.mode !== 'single' || trig.status === 'stop')) return trig.shown;
        await sleepMs(50);
      }
      return trig.shown;
    },
    dual(t0, t1) {
      view.hold(store);
      zoomToMarker(view.addDual(t0, t1).id);
    },
    clearMarkers: () => view.clearMarkers(),
    analyze(tab) {
      if (!tab) {
        analysis = null;
        return;
      }
      openAnalysis();
      if (analysis) analysis = { ...analysis, tab, autorun: true };
    },
    meter: (sc) => void (meterScope = sc),
    bottomTab: (t) => void (bottomTab = t),
    openSection: (id) => void dispatchEvent(new CustomEvent('nebulosa:open-section', { detail: id })),
    autoset: () => autoset(),
    async newSession() {
      await activeLink?.clearSession();
      lv = { follow: true, last: 0, t0: 0, t1: 0 };
    },
    serialFeed: (on) => (on ? startDemoFeed() : stopDemoFeed()),
    scaOpen: () => setAppMode('sca'),
    scaCapture: async (plan, t, sLen) => {
      if (await whenSca()) await sca!.scaCapture(plan, t, sLen);
    },
    scaView: (v) => sca?.scaSetView(v),
    scaAlign: async (a, b) => {
      if (await whenSca()) await sca!.scaAlign(a, b);
    },
    scaRecoverKey: async () => {
      if (await whenSca()) await sca!.scaRecoverKey();
    },
    look: {
      markers: () => view.markers,
      active: () => view.active,
      live: () => view.live,
      divIndex: () => view.divIndex,
      end: () => view.end,
      trig,
      bottomTab: () => bottomTab,
      meterScope: () => meterScope,
      appMode: () => appMode,
      mathOn: () => mathInfo.on,
      mathExpr: () => mathInfo.expr,
      logView: () => lv,
      analysisOpen: () => !!analysis,
      serialTriggerOn: () => serialCfg.trigger.on && !!serialCfg.trigger.pattern,
      serialMatches: () => monitor.matches,
      serialMsgs: () => monitor.annotations.length,
      scaTraces: () => sca?.scaState().traces ?? 0,
      scaView: () => sca?.scaState().view ?? 'traces',
      scaCapturing: () => sca?.scaState().capturing ?? false,
      scaTtestMax: () => sca?.scaState().ttestMax ?? 0,
      scaAligned: () => sca?.scaState().aligned ?? false,
      scaKeyDone: () => sca?.scaState().keyDone ?? false,
      scaKeyCorrect: () => sca?.scaState().keyCorrect ?? false,
    },
  };

  async function runShowcase(s: Showcase) {
    showcasesOpen = false;
    touring = false;
    showcase = null;
    stopDemoFeed();
    analysis = null;
    if (dev && !(dev.link instanceof DemoLink)) await disconnect();
    if (!dev) startDemo();
    else if (!streaming) await start();
    (dev!.link as unknown as DemoLink).setScenario(s.scenario);
    scenarioSince = store.empty ? 0 : store.tEnd;
    // each starts from a plain scope: no math, no trigger, no event log, markers in view
    if (mathInfo.on) await mathOff();
    Object.assign(trig, { mode: 'roll', log: false });
    view.clearMarkers();
    bottomTab = 'markers';
    meterScope = 'window';
    leftOpen = rightOpen = true;
    resetView();
    await tick();
    showcase = { s, steps: s.steps(showcaseCtx) };
  }

  function showSpan(t0: number, t1: number) {
    view.hold(store);
    const m = view.addDual(t0, t1);
    analysis = null;
    zoomToMarker(m.id);
  }

  // ---- sidebar widths --------------------------------------------------------
  const LEFT_W = 280, RIGHT_W = 280;
  let leftW = $state(stored('nebulosa.leftW', LEFT_W, (v) => v >= 200 && v <= 600));
  /** the sidebars shown (hidden: more room for the plots) */
  let leftOpen = $state(stored('nebulosa.leftOpen', 1, (v) => v === 0 || v === 1) === 1);
  let rightOpen = $state(stored('nebulosa.rightOpen', innerWidth < 1100 ? 0 : 1, (v) => v === 0 || v === 1) === 1);
  function toggleSide(side: 'left' | 'right') {
    if (side === 'left') save('nebulosa.leftOpen', (leftOpen = !leftOpen) ? 1 : 0);
    else save('nebulosa.rightOpen', (rightOpen = !rightOpen) ? 1 : 0);
  }
  let rightW = $state(stored('nebulosa.rightW', RIGHT_W, (v) => v >= 200 && v <= 600));

  function colDrag(side: 'left' | 'right') {
    let w0 = 0;
    return {
      start: () => (w0 = side === 'left' ? leftW : rightW),
      move: (dx: number) => {
        const w = Math.round(Math.min(600, Math.max(200, w0 + (side === 'left' ? dx : -dx))));
        if (side === 'left') leftW = w;
        else rightW = w;
      },
      end: () => save(side === 'left' ? 'nebulosa.leftW' : 'nebulosa.rightW', side === 'left' ? leftW : rightW),
    };
  }

  // ---- splitter between the plots and the lists ----------------------------
  const SPLIT_KEY = 'nebulosa.listHeight';
  function loadSplit() {
    try {
      const v = Number(localStorage.getItem(SPLIT_KEY));
      if (v >= 60) return v;
    } catch {
      /* storage unavailable */
    }
    return 190;
  }
  let listH = $state(loadSplit());
  let mainEl: HTMLElement;

  function startSplit(e: PointerEvent) {
    e.preventDefault();
    const el = e.currentTarget as HTMLElement;
    el.setPointerCapture(e.pointerId);
    const y0 = e.clientY;
    const h0 = listH;
    // keep the plots at least ~200 px and the list at least one row
    const max = Math.max(80, mainEl.clientHeight - 260);
    const move = (ev: PointerEvent) => (listH = Math.round(Math.min(max, Math.max(60, h0 - (ev.clientY - y0)))));
    const up = () => {
      el.removeEventListener('pointermove', move);
      el.removeEventListener('pointerup', up);
      el.removeEventListener('pointercancel', up);
      try {
        localStorage.setItem(SPLIT_KEY, String(listH));
      } catch {
        /* storage unavailable */
      }
    };
    el.addEventListener('pointermove', move);
    el.addEventListener('pointerup', up);
    el.addEventListener('pointercancel', up);
  }

  async function applyBuffer() {
    try {
      localStorage.setItem(BUFFER_KEY, String(bufferMB));
    } catch {
      /* storage unavailable */
    }
    await dev?.link.setBuffer(bufferSamples);
  }

  // ---- math channel ---------------------------------------------------------
  const MATH_KEY = 'nebulosa.math';
  let mathDraft = $state({ name: 'Power', unit: 'mW', expr: 'P*1000' });
  let mathError = $state('');
  try {
    const m = JSON.parse(localStorage.getItem(MATH_KEY) ?? 'null');
    if (m && typeof m.expr === 'string') {
      mathDraft = { name: m.name ?? '', unit: m.unit ?? '', expr: m.expr };
      if (m.on) Object.assign(mathInfo, { name: m.name ?? '', unit: m.unit ?? '', expr: m.expr, on: true });
    }
  } catch {
    /* storage unavailable */
  }

  function saveMath() {
    save(MATH_KEY, { ...$state.snapshot(mathDraft), on: mathInfo.on });
  }


  async function applyMath() {
    mathError = '';
    const err = activeLink ? await activeLink.setMath(mathDraft.expr) : null;
    if (err) {
      mathError = err;
      return;
    }
    Object.assign(mathInfo, { ...$state.snapshot(mathDraft), on: true });
    shown.m = true;
    saveMath();
  }

  async function mathOff() {
    await activeLink?.setMath(null);
    mathInfo.on = false;
    mathError = '';
    saveMath();
  }

  /** a new link gets the math channel too */
  async function linkMath(l: Link) {
    if (!mathInfo.on) return;
    const err = await l.setMath(mathInfo.expr);
    if (err) {
      mathError = err;
      mathInfo.on = false;
    }
  }

  // ---- statistics ---------------------------------------------------------
  function summarize(s: SpanStats, duration = s.t1 - s.t0): Summary | null {
    if (!s.i.n) return null;
    const ch = (m: SpanStats['i']) => ({ mean: m.mean, min: m.min, max: m.max, pp: m.pp, std: m.std, rms: m.rms });
    return { ch: { i: ch(s.i), v: ch(s.v), p: ch(s.p), m: ch(s.m) }, n: s.i.n, duration };
  }

  let meterBusy = false;

  /** what the meter's last answers were for: the same again needs no new queries */
  let meterKey = '';

  /** the meter's inputs: the span shown and the data in it (a held span's data only changes when the buffer drops its start) */
  function meterInputs() {
    const marks = view.markers.map((m) => `${m.id}:${m.t0}:${m.t1}`).join(',');
    if (meterScope === 'run') return `run|${streamId}|${runMoments?.i.n}|${marks}|${store.version}`;
    if (appMode === 'logger') {
      const [a, b] = loggerSpan();
      return `log|${a}|${b}|${store.session.version}|${marks}|${store.version}`;
    }
    const sp = meterScope === 'selection' && view.selection ? view.selection : view.span(store);
    const data = view.live || store.tStart > sp[0] ? store.version : 'held';
    // and at least every 2 s anyway (calibration, math: things not in this key)
    return `${meterScope}|${appMode}|${sp[0]}|${sp[1]}|${data}|${marks}|${Math.floor(Date.now() / 2000)}`;
  }

  async function refreshMeter() {
    if (hold || meterBusy) return;
    const key = meterInputs();
    if (key === meterKey) {
      updateAccumView();
      return;
    }
    meterBusy = true;
    try {
      if (meterScope === 'run') {
        const r = runMoments;
        summary = r && r.i.n ? summarize({ i: Moments.from(r.i), v: Moments.from(r.v), p: Moments.from(r.p), m: Moments.from(r.m), t0: 0, t1: 0, charge: 0, energy: 0 }, accum?.seconds ?? NaN) : null;
      } else if (appMode === 'logger') summary = store.session.has_data ? summarize(await store.sessionStats(...loggerSpan())) : null;
      else if (meterScope === 'selection' && view.selection) summary = summarize(await store.stats(...view.selection));
      else if (store.empty) summary = null;
      else summary = summarize(await store.stats(...view.span(store)));
      markerRows = await Promise.all(
        view.markers.map(async (m): Promise<MarkerRow & { t0: number }> => {
          const base = { id: m.id, name: markerName(m), color: markerColor(m), t0: m.t0 };
          if (m.t1 == null) {
            const v = await store.valueAt(m.t0);
            return { ...base, dt: null, ch: Object.fromEntries(SIGNALS.map((s) => [s, v ? { mean: v[s], integral: NaN } : null])) as MarkerRow['ch'] };
          }
          const st = await store.stats(m.t0, m.t1);
          const integral: Record<Signal, number> = { i: st.charge, v: NaN, p: st.energy, m: NaN };
          return {
            ...base,
            dt: m.t1 - m.t0,
            ch: Object.fromEntries(SIGNALS.map((s) => [s, st[s].n ? { mean: st[s].mean, integral: integral[s] } : null])) as MarkerRow['ch'],
          };
        }),
      );
      updateAccumView();
      meterKey = key;
    } catch (e) {
      console.warn('meter', e);
    } finally {
      meterBusy = false;
    }
  }

  function updateAccumView() {
    accumView = accum
      ? {
          charge: accum.charge - accumBase.charge,
          energy: accum.energy - accumBase.energy,
          seconds: accum.seconds - accumBase.seconds,
        }
      : null;
  }

  function clearAccum() {
    if (accum) accumBase = { ...accum };
    meterKey = '';
    refreshMeter();
  }

  // a new selection shows its statistics; clearing it falls back to the window
  $effect(() => {
    if (view.selection) meterScope = 'selection';
    else if (meterScope === 'selection') meterScope = 'window';
  });

  $effect(() => {
    void meterScope;
    void hold;
    refreshMeter();
  });

  function clearData() {
    store.clear();
    runMoments = null;
    view.clearMarkers();
    view.live = true;
    summary = null;
  }

  // ---- view controls ------------------------------------------------------
  function toggleLive() {
    if (view.live) view.hold(store);
    else view.live = true;
  }

  function scaleMenu(e: MouseEvent) {
    openMenu(e, [
      { label: 'Configurations…', run: () => (configOpen = true) },
      { sep: true },
      { head: canScale ? 'UI scale' : "UI scale: the browser's zoom" },
      ...SCALES.map((v) => ({
        label: `${Math.round(v * 100)} %`,
        hint: v === 1 ? 'Ctrl 0' : undefined,
        checked: canScale && uiScale.value === v,
        disabled: !canScale,
        run: () => setScale(v),
      })),
      { sep: true },
      { label: canScale ? 'Bigger / smaller: Ctrl + / Ctrl −' : 'Use Ctrl + / Ctrl − / Ctrl 0', disabled: true },
      { sep: true },
      { head: 'Theme' },
      { label: 'Dark', checked: uiTheme.value === 'dark', run: () => setTheme('dark') },
      { label: 'Light', checked: uiTheme.value === 'light', run: () => setTheme('light') },
      { label: "The system's", checked: uiTheme.value === 'system', run: () => setTheme('system') },
    ]);
  }

  function onKey(e: KeyboardEvent) {
    // Ctrl / ⌘ + − 0: the UI scale (the browser zooms by itself)
    if (e.ctrlKey || e.metaKey) {
      // Ctrl / ⌘ + K: find a setting or an action
      if (e.key === 'k' || e.key === 'K') {
        e.preventDefault();
        paletteOpen = true;
        return;
      }
      // Ctrl / ⌘ + 1 / 2 / 3: Scope / Logger / Side-channel
      if (e.key === '1' || e.key === '2' || e.key === '3') {
        setAppMode(e.key === '2' ? 'logger' : e.key === '3' ? 'sca' : 'scope');
        e.preventDefault();
        return;
      }
      if (!canScale) return;
      if (e.key === '=' || e.key === '+') stepScale(1);
      else if (e.key === '-' || e.key === '_') stepScale(-1);
      else if (e.key === '0') setScale(1);
      else return;
      e.preventDefault();
      return;
    }
    const tag = (e.target as HTMLElement).tagName;
    if (tag === 'INPUT' || tag === 'SELECT' || tag === 'TEXTAREA') return;
    // Alt + ← / →: back / forward through the views shown
    if (e.altKey && (e.key === 'ArrowLeft' || e.key === 'ArrowRight')) {
      e.preventDefault();
      if (e.key === 'ArrowLeft') navBack();
      else navForward();
      return;
    }
    // everywhere
    if (e.key === '?' || e.key === 'F1') {
      e.preventDefault();
      openHelp('start');
      return;
    }
    if (e.key === '[') return toggleSide('left');
    if (e.key === ']') return toggleSide('right');
    if (e.key === 'Escape') {
      if (pickTrigger) pickTrigger = false;
      else view.active = null;
      return;
    }
    // the rest moves the view on screen: the Logger's keys in the Logger, the Scope's in the Scope
    if (appMode === 'sca') return;
    const page = e.shiftKey ? 1 : 0.1;
    if (appMode === 'logger') {
      if (e.key === ' ') {
        e.preventDefault();
        toggleFollow();
      } else if (e.key === 'ArrowLeft') loggerPan(-page);
      else if (e.key === 'ArrowRight') loggerPan(page);
      else if (e.key === '+' || e.key === '=') loggerZoom(1 / 1.5);
      else if (e.key === '-') loggerZoom(1.5);
      else if (e.key === 'Home') loggerHome();
      else if (e.key === 'End') lv = { ...lv, follow: true };
      else if (e.key === ',') stepSerial(-1);
      else if (e.key === '.') stepSerial(1);
      else return;
      e.preventDefault();
      return;
    }
    if (e.key === ' ') {
      e.preventDefault();
      toggleLive();
    } else if (e.key === 'Delete' || e.key === 'Backspace') view.remove(view.active);
    else if (e.key === 'm') addSingleMarker();
    else if (e.key === 'd') addDualMarker();
    else if (e.key === 'a' && hasData) openAnalysis();
    else if (e.key === 'l') showInLogger(scopeRef());
    else if (e.key === 'n') stepEvent(1);
    else if (e.key === 'N') stepEvent(-1);
    else if (e.key === '.') stepSerial(1);
    else if (e.key === ',') stepSerial(-1);
    else if (e.key === '>') stepMarker(1);
    else if (e.key === '<') stepMarker(-1);
    else if (e.key === '+' || e.key === '=') view.zoom(store, -1);
    else if (e.key === '-') view.zoom(store, 1);
    else if (e.key === 'ArrowLeft') view.pan(store, -page);
    else if (e.key === 'ArrowRight') view.pan(store, page);
    else if (e.key === 'Home') scopeHome();
    else if (e.key === 'End') view.live = true;
    else return;
    if (e.key !== ' ') e.preventDefault();
  }

  /** the mouse's back / forward buttons: through the views shown */
  function onMouseNav(e: MouseEvent) {
    if (e.button !== 3 && e.button !== 4) return;
    e.preventDefault();
    if (e.button === 3) navBack();
    else navForward();
  }

  /** fits the screen to a marker: a dual one fills ~80 % of it, a single one is centred */
  /** a serial message's moment in the Scope: held, at the trigger position */
  function jumpToSerial(t: number) {
    view.hold(store);
    view.end = t + (1 - trig.position) * view.windowS;
  }
  /** a serial message's moment in the Logger: centred, the current span kept */
  function jumpLoggerTo(t: number) {
    const s0 = store.session;
    const w = lv.follow ? (lv.last > 0 ? lv.last : Math.max(1, s0.t_end - s0.t_start)) : lv.t1 - lv.t0;
    lv = { follow: false, last: lv.last, t0: t - w / 2, t1: t + w / 2 };
  }
  /** the newest sample's time, reactive (the store's isn't) */
  let tEndNow = $state(0);
  /** what relative times in the Scope's lists count from: the trigger shown, else the screen's right edge */
  const scopeOrigin = $derived(trig.mode !== 'roll' && trig.shown != null ? trig.shown : view.live ? tEndNow : view.end);
  let loggerMsgsOpen = $state(stored('nebulosa.loggerMsgs', 1) === 1);
  $effect(() => save('nebulosa.loggerMsgs', loggerMsgsOpen ? 1 : 0));

  /** clear the markers, with an Undo toast */
  function clearMarkersUndoable() {
    if (!view.markers.length) return;
    const snapshot = view.markers.map((m) => ({ ...m }));
    view.clearMarkers();
    toast(`Cleared ${snapshot.length} marker${snapshot.length > 1 ? 's' : ''}`, { label: 'Undo', run: () => view.restore(snapshot) });
  }

  function renameMarker(id: number) {
    const m = view.markers.find((x) => x.id === id);
    if (!m) return;
    const name = prompt('Marker name (blank to reset):', m.name ?? '');
    if (name != null) view.setName(id, name);
  }

  /** a marker's measurements as tab-separated text, to the clipboard */
  async function copyMarker(id: number) {
    const row = markerRows.find((r) => r.id === id);
    if (!row) return;
    const cols = [markerName({ id, t0: row.t0, t1: row.dt == null ? null : row.t0 + row.dt })];
    if (row.dt != null) cols.push(`Δt ${fmtTime(row.dt)}`);
    for (const s of SIGNALS) {
      const c = row.ch[s];
      if (c) cols.push(`${SIGNAL_INFO[s].name} ${fmtSI(c.mean, SIGNAL_INFO[s].unit, 5)}${row.dt != null && isFinite(c.integral) ? ` ∫ ${fmtSI(c.integral, SIGNAL_INFO[s].integral ?? '', 4)}` : ''}`);
    }
    try {
      await navigator.clipboard.writeText(cols.join('\t'));
      toast('Marker values copied');
    } catch {
      toast('Could not copy');
    }
  }

  function zoomToMarker(id: number) {
    const m = view.markers.find((x) => x.id === id);
    if (!m) return;
    view.active = id;
    view.hold(store);
    const len = m.t1 == null ? view.windowS : (m.t1 - m.t0) / 0.8;
    let k = TIME_DIVS.findIndex((d) => d * 10 >= len);
    if (k < 0) k = TIME_DIVS.length - 1;
    view.divIndex = k;
    const mid = m.t1 == null ? m.t0 : (m.t0 + m.t1) / 2;
    view.end = mid + view.windowS / 2;
  }

  // ---- right-click menus --------------------------------------------------------
  function analyzeMarker(id: number) {
    view.active = id;
    openAnalysis();
  }

  function exportMarker(id: number) {
    view.active = id;
    exportCsv();
  }

  /** what can be done with one marker */
  function markerItems(id: number): MenuEntry[] {
    const m = view.markers.find((x) => x.id === id);
    if (!m) return [];
    const dual = m.t1 != null;
    return [
      { head: dual ? `${markerName(m)} · Δt ${fmtTime(m.t1! - m.t0)}` : markerName(m) },
      { label: 'Zoom to it', run: () => zoomToMarker(id) },
      { label: 'Show in the Logger', disabled: !store.session.has_data, run: () => showInLogger(m.t1 == null ? m.t0 : (m.t0 + m.t1) / 2) },
      ...(dual
        ? [
            { label: 'Analyze…', run: () => analyzeMarker(id) },
            ...(isNative ? [{ label: 'Export samples to CSV…', run: () => exportMarker(id) }] : []),
          ]
        : []),
      { label: 'Rename…', run: () => renameMarker(id) },
      { label: 'Copy values', run: () => copyMarker(id) },
      { label: 'Delete', hint: 'Del', danger: true, run: () => view.remove(id) },
    ];
  }

  function allMarkerItems(): MenuEntry[] {
    const n = view.markers.length;
    const duals = view.markers.filter((m) => m.t1 != null).length;
    return [
      { label: `Delete all markers${n ? ` (${n})` : ''}`, danger: true, disabled: !n, run: clearMarkersUndoable },
      ...(duals && duals < n
        ? [
            { label: `Delete the dual markers (${duals})`, danger: true, run: () => view.markers.filter((m) => m.t1 != null).forEach((m) => view.remove(m.id)) },
            { label: `Delete the single markers (${n - duals})`, danger: true, run: () => view.markers.filter((m) => m.t1 == null).forEach((m) => view.remove(m.id)) },
          ]
        : []),
    ];
  }

  /** puts the trigger on `sig` at `v` (a window: the same width, centred on v) */
  let pickTrigger = $state(false);
  /** click-to-set: a plot click sets the trigger source and level there */
  function onPickTrigger(spot: ScopeSpot) {
    if (spot.sig && spot.value != null && (!axes[spot.sig].log || spot.value > 0)) {
      triggerAt(spot.sig, spot.value);
      toast(`Trigger: ${SIGNAL_INFO[spot.sig].name} at ${fmtSI(spot.value, SIGNAL_INFO[spot.sig].unit, 3)}`);
    }
    pickTrigger = false;
  }

  function triggerAt(sig: Signal, v: number) {
    trig.source = sig;
    if (trig.kind === 'window') {
      const h = Math.abs(trig.high - trig.low) / 2 || Math.abs(v) * 0.1 || 1e-6;
      trig.low = v - h;
      trig.high = v + h;
    } else trig.level = v;
    if (trig.mode === 'roll') trig.mode = 'auto';
  }

  /** HOLD where the screen was at the right-click (LIVE moves on while the menu is open) */
  function holdAt(at: ScopeSpot) {
    if (!view.live) return;
    view.live = false;
    view.end = at.end;
  }

  function scopeMenu(e: MouseEvent, at: ScopeSpot) {
    const items: MenuEntry[] = [];
    if (at.marker) items.push(...markerItems(at.marker.id), { sep: true });
    if (at.sig && hasData) {
      const info = SIGNAL_INFO[at.sig];
      const w = view.timeDiv;
      items.push(
        { label: 'Add marker here', hint: 'dbl-click', run: () => (holdAt(at), view.addSingle(at.t)) },
        { label: 'Add dual marker here', run: () => (holdAt(at), view.addDual(at.t - w / 2, at.t + w / 2)) },
      );
      if (at.value != null && (!axes[at.sig].log || at.value > 0))
        items.push({ label: `Trigger on ${info.name.toLowerCase()} at ${fmtSI(at.value, info.unit, 3)}`, run: () => triggerAt(at.sig!, at.value!) });
      items.push({ sep: true });
    }
    items.push(
      { label: view.live ? 'Hold' : 'Live', hint: 'Space', run: () => (view.live ? holdAt(at) : (view.live = true)) },
      { label: 'Zoom in here', hint: '+', disabled: view.divIndex === 0, run: () => (holdAt(at), view.zoom(store, -1, at.t)) },
      { label: 'Zoom out', hint: '−', disabled: view.divIndex === TIME_DIVS.length - 1, run: () => view.zoom(store, 1, at.t) },
      { label: 'Autoset', disabled: !hasData, run: autoset },
      { label: 'Show in the Logger', hint: 'L', disabled: !store.session.has_data, run: () => showInLogger(at.t) },
      { label: 'Back', hint: 'Alt ←', disabled: !scopeHist.canBack, run: navBack },
      { label: 'Reset view', run: resetView },
    );
    if (at.sig) {
      const sig = at.sig, ax = axes[sig], range = at.range;
      items.push(
        { sep: true },
        { head: `${SIGNAL_INFO[sig].name} axis` },
        { label: 'Auto range', checked: ax.auto, run: () => (ax.auto = true) },
        {
          label: 'Fixed at this range',
          checked: !ax.auto,
          disabled: !range,
          run: () => {
            if (!range) return;
            [ax.min, ax.max] = range;
            ax.auto = false;
          },
        },
        { label: 'Logarithmic', checked: ax.log, run: () => (ax.log = !ax.log) },
        { label: `Hide ${SIGNAL_INFO[sig].name.toLowerCase()}`, disabled: signals.length < 2, run: () => (shown[sig] = false) },
      );
    }
    items.push(
      { sep: true },
      ...allMarkerItems(),
      { sep: true },
      { label: view.selection ? 'Analyze the active marker…' : 'Analyze the screen…', disabled: !hasData, run: openAnalysis },
      { label: 'Save as PNG…', disabled: !hasData, run: exportPng },
    );
    openMenu(e, items);
  }

  function markerListMenu(e: MouseEvent, id: number | null) {
    if (id != null) view.active = id;
    openMenu(e, [
      ...(id != null ? markerItems(id) : []),
      { sep: true },
      { label: 'Add marker', hint: 'M', disabled: !hasData, run: addSingleMarker },
      { label: 'Add dual marker', hint: 'D', disabled: !hasData, run: addDualMarker },
      { sep: true },
      ...allMarkerItems(),
    ]);
  }

  function eventMenu(e: MouseEvent, ev: TrigEvent | null) {
    openMenu(e, [
      ...(ev
        ? [
            { head: `Event ${ev.seq + 1} · ${EVENT_LABEL[ev.what]}` },
            { label: 'Show it', run: () => jumpToEvent(ev) },
            {
              label: 'Mark it (single marker)',
              run: () => (jumpToEvent(ev), view.addSingle(ev.t)),
            },
            ...(ev.width != null && ev.width > 0
              ? [{ label: 'Mark its width (dual marker)', run: () => (jumpToEvent(ev), view.addDual(ev.t - ev.width!, ev.t)) }]
              : []),
            { sep: true } as const,
          ]
        : []),
      { label: 'Export events to CSV…', disabled: !trig.count, run: exportEvents },
      { label: 'Clear events', danger: true, disabled: !trig.count, run: () => trig.clearLog(activeLink) },
    ]);
  }

  function addSingleMarker() {
    if (store.empty) return;
    view.hold(store);
    const [a, b] = view.span(store);
    view.addSingle((a + b) / 2);
  }

  function addDualMarker() {
    if (store.empty) return;
    view.hold(store);
    const [a, b] = view.span(store);
    const w = b - a;
    view.addDual(a + w * 0.4, a + w * 0.6);
  }

  // ---- connection ---------------------------------------------------------
  async function scan() {
    scanning = true;
    try {
      candidates = isNative ? await native.scan() : await web.scan();
    } catch (e) {
      error = String(e);
    } finally {
      scanning = false;
    }
  }

  async function pair() {
    const c = await web.pick();
    if (c) await connect(c);
  }

  let lastCandidate = '';
  // ---- losing the link: reconnect, and carry on streaming / logging ----------
  let reconnecting = $state(false);

  async function linkLost(c: Candidate, reason: string, resume: { stream: boolean; log: boolean }) {
    reconnecting = true;
    error = '';
    note = `Connection lost (${reason}). Reconnecting…`;
    const t0 = Date.now();
    while (reconnecting && Date.now() - t0 < 120_000) {
      await new Promise((r) => setTimeout(r, 2000));
      if (!reconnecting || dev) break;
      await scan();
      // a device found again by its key; a host typed by hand isn't scanned
      const again = candidates.find((x) => x.key === c.key) ?? (c.key.startsWith('tcp:') ? c : null);
      if (!again) continue;
      await connect(again, true);
      if (!dev) continue;
      reconnecting = false;
      if (resume.stream) await start();
      if (resume.log) await startLog();
      error = '';
      note = `Reconnected after ${((Date.now() - t0) / 1000).toFixed(0)} s${resume.log ? '; the log goes on in a new file' : ''}.`;
      return;
    }
    if (reconnecting) {
      error = `Disconnected: ${reason}. Not back after 2 minutes; connect again when it is.`;
      note = '';
    }
    reconnecting = false;
  }

  async function connect(c: Candidate, retry = false) {
    lastCandidate = c.key;
    busy = true;
    if (!retry) {
      error = '';
      reconnecting = false;
    }
    try {
      await disconnect();
      const link: Link = await c.connect({
        onMeta,
        onClosed: (reason) => {
          const resume = { stream: streaming && !link.playback, log: !!(logging?.active || exporting) };
          const live = !link.playback && dev?.link === link;
          reset();
          if (live) linkLost(c, reason, resume);
          else error = `Disconnected: ${reason}`;
        },
      });
      const d = new Device(link);
      dev = d;
      await link.setBuffer(bufferSamples);
      await link.setCalibration($state.snapshot(cal));
      await link.setTrigger(trig.cfg());
      await linkMath(link);
      await link.setLogInterval(logInterval);
      store.attach(link);
      setLink(link);
      info = await d.info();
      // a device left streaming (the app closed or the cable pulled while it ran)
      // keeps sending: put it in a known, stopped state; Start runs it
      if (!link.playback) await d.stop().catch(() => undefined);
      streaming = false;
      clearData();
      await applyDecimation();
    } catch (e) {
      if (!retry) error = String(e);
      await disconnect();
    } finally {
      busy = false;
    }
  }

  function reset() {
    // another device (or none): the views before don't apply to it
    scopeHist.clear();
    loggerHist.clear();
    hasData = false;
    store.attach(null);
    setLink(null);
    dev = null;
    playback = null;
    recording = null;
    info = null;
    streaming = false;
    stats = null;
    throughput = 0;
  }

  /** a synthetic device (2 MS/s, a periodic load): the whole app without hardware */
  function startDemo() {
    reset();
    streaming = true;
    rateHz = 2e6;
    log2n = 0;
    const demo = new DemoLink({ onMeta, onClosed: () => undefined });
    dev = new Device(demo);
    info = {
      proto_version: 1, fw_version: 0x200, fw_version_str: '0.2.0', fpga_id: 0x4e42000b, base_rate_hz: 2_000_000,
      link: 3, link_name: 'Demo', link_unit: 2048, block_words: 507, caps: CAP.DECIM | CAP.RANGE | CAP.BOARD,
      build: 'synthetic', mac: '', ip: '',
    };
    demo.setBuffer(bufferSamples);
    demo.setCalibration($state.snapshot(cal));
    demo.setTrigger(trig.cfg());
    linkMath(demo);
    demo.setLogInterval(logInterval);
    store.attach(demo);
    setLink(demo);
  }

  async function disconnect() {
    const d = dev;
    const wasStreaming = streaming;
    reset();
    if (d && wasStreaming && !d.link.playback) await d.stop().catch(() => undefined);
    if (d) await d.link.close().catch(() => undefined);
  }

  function onMeta(m: Meta) {
    store.update(m);
    tEndNow = m.t_end;
    sess = m.session;
    logging = m.logging;
    if (wallPending != null && (m.session.has_data || m.has_data)) {
      wallOffset = wallPending - (m.session.has_data ? m.session.t_start : m.t_start);
      wallPending = null;
    } else if (!dev?.link.playback && m.has_data && Math.abs(Date.now() / 1000 - m.t_end - wallOffset) > 2) {
      // live: wall clock = now at the newest sample (re-anchored if the FPGA clock restarted)
      wallOffset = Date.now() / 1000 - m.t_end;
    }
    if (hasData !== m.has_data) hasData = m.has_data;
    recording = m.recording;
    if (fitPending && !store.empty) {
      fitPending = false;
      fitAll();
    }
    trig.update(m.trigger.recent, m.trigger.count, store, view);
    trig.fetch(activeLink, m.trigger.count);
    runMoments = m.run;
    stats = m.stats;
    if (m.stream_id !== streamId) {
      streamId = m.stream_id;
      accumBase = { charge: 0, energy: 0, seconds: 0 };
    }
    accum = m.accum;
    const now = performance.now();
    if (now - lastBytes.t > 1000) {
      if (lastBytes.t && m.stats.bytes >= lastBytes.bytes)
        throughput = ((m.stats.bytes - lastBytes.bytes) * 1000) / (now - lastBytes.t);
      lastBytes = { t: now, bytes: m.stats.bytes };
    }
  }

  async function run(f: () => Promise<unknown>) {
    error = '';
    try {
      await f();
    } catch (e) {
      error = String(e);
    }
  }

  async function applyDecimation() {
    if (!dev || !info) return;
    if (info.caps & CAP.DECIM) {
      const r = await dev.setDecimation(log2n, decimMode);
      rateHz = r.rate_hz;
    } else rateHz = info.base_rate_hz;

  }

  async function start() {
    if (!dev) return;
    await run(async () => {
      await dev!.setSource(source);
      if (info!.caps & CAP.RANGE) await dev!.setRange(rangeAuto, rangeManual, rangeMin, rangeMax);
      await applyDecimation();
      clearData();
      lastBytes = { t: 0, bytes: 0 };
      await dev!.start();
      streaming = true;
    });
  }

  async function stop() {
    if (!dev) return;
    await run(() => dev!.stop());
    streaming = false;
  }



  function fmtBytes(bps: number) {
    return bps >= 1e6 ? `${(bps / 1e6).toFixed(2)} MB/s` : `${(bps / 1e3).toFixed(0)} kB/s`;
  }

  onMount(() => {
    restoreScale();
    // a default configuration applied on boot (not during screenshot runs)
    if (!demoRequested) {
      const def = defaultConfigName();
      const c = def ? listConfigs()[def] : null;
      if (c) {
        const cfg = asConfig(c);
        if (cfg) applyConfig(cfg);
      }
    }
    scan();
    refreshRecordings();
    // drag a recording / log onto the window to open it (desktop app)
    let dropOff: (() => void) | undefined;
    if (isNative) {
      import('@tauri-apps/api/webview').then(({ getCurrentWebview }) => {
        getCurrentWebview()
          .onDragDropEvent((e) => {
            if (e.payload.type !== 'drop') return;
            const path = e.payload.paths.find((p) => /\.(nbrec|nblog)$/i.test(p));
            if (path) openRecording(path);
          })
          .then((un) => (dropOff = un))
          .catch(() => undefined);
      });
    }
    const t = setInterval(refreshMeter, 250);
    const wd = setInterval(watchdog, 2000);
    // the first run: the walkthrough (not in demo / screenshot mode)
    if (!demoRequested && !tourSeen()) setTimeout(startTour, 600);
    if (demoRequested) {
      markTourSeen();
      startDemo();
      // screenshot helpers: &log=i,p  &trace=mean,range,sigma
      const q = new URLSearchParams(location.search);
      for (const sg of (q.get('log') ?? '').split(',')) if (sg in axes) axes[sg as Signal].log = true;
      const tg = q.get('trig');
      if (tg) {
        const [src, slope, level, mode, pos] = tg.split(',');
        trig.source = src as Signal;
        if (slope === 'pulse' || slope === 'window' || slope === 'timeout') trig.kind = slope;
        else trig.slope = slope as typeof trig.slope;
        trig.level = Number(level);
        trig.mode = mode as TrigMode;
        if (pos) trig.position = Number(pos);
      }
      if (q.get('tab') === 'events') bottomTab = 'events';
      if (q.get('mode') === 'logger') appMode = 'logger';
      if (q.get('mode') === 'scope') appMode = 'scope';
      const mx = q.get('math');
      if (mx) {
        mathDraft = { name: q.get('mname') ?? 'Math', unit: q.get('munit') ?? '', expr: mx };
        setTimeout(applyMath, 0);
      }
      const div = Number(q.get('div'));
      if (div && TIME_DIVS.includes(div)) view.divIndex = TIME_DIVS.indexOf(div);
      const tr = q.get('trace');
      if (tr) trace = { mean: tr.includes('mean'), range: tr.includes('range'), sigma: tr.includes('sigma') };
      const sel = demoSelection();
      if (sel)
        setTimeout(() => {
          view.hold(store);
          view.addDual(store.tEnd - sel[0], store.tEnd - sel[1]);
          view.addSingle(store.tEnd - (sel[0] + sel[1]) / 2 - 1);
          view.addDual(store.tEnd - sel[0] + 3, store.tEnd - sel[1] + 3.2);
        }, 3000);
    }
    return () => {
      clearInterval(t);
      clearInterval(wd);
      dropOff?.();
      dev?.link.close();
    };
  });
</script>

<svelte:window onkeydown={onKey} onmouseup={onMouseNav} />
<ContextMenu />

{#if updating && dev}
  <Updater {dev} onclose={() => (updating = false)} onrestart={() => { updating = false; boardRestarts(); }} />
{/if}

{#if paletteOpen}
  <CommandPalette commands={commands()} onclose={() => (paletteOpen = false)} />
{/if}
{#if helpOpen}
  <Help bind:tab={helpTab} {info} link={dev?.link.description ?? ''} onclose={() => (helpOpen = false)} ontour={startTour} />
{/if}

{#if touring}
  <Tour steps={TOUR} connected={!!dev} ondemo={startDemo} onclose={() => (touring = false)} />
{/if}

{#if configOpen}
  <ConfigDialog gather={gatherConfig} onapply={applyConfig} onclose={() => (configOpen = false)} />
{/if}

{#if showcasesOpen}
  <Showcases
    realDevice={dev && !(dev.link instanceof DemoLink) ? dev.link.description : null}
    onpick={runShowcase}
    ontour={() => {
      showcasesOpen = false;
      startTour();
    }}
    onclose={() => (showcasesOpen = false)}
  />
{/if}

{#if showcase}
  {#key showcase}
    <Tour steps={showcase.steps} label={showcase.s.title} interactive connected={!!dev} onclose={() => { showcase = null; stopDemoFeed(); }} />
  {/key}
{/if}

{#if analysis}
  {#key analysis.tab}
    <Analysis
      signals={available}
      link={activeLink}
      span={analysis.span}
      spanLabel={analysis.label}
      initialTab={analysis.tab}
      autorun={analysis.autorun}
      onclose={() => (analysis = null)}
      onshow={showSpan}
    />
  {/key}
{/if}

<div class="app" style="grid-template-columns: {leftOpen ? leftW : 0}px minmax(0, 1fr) {rightOpen ? rightW : 0}px">
  <!-- column splitters: on the grid, so the sidebars' scrolling doesn't move or clip them -->
  {#if leftOpen}
  <div
    class="vsplit"
    style="left: {leftW - 4}px"
    role="separator"
    aria-orientation="vertical"
    title="Drag to resize; double-click to reset"
    use:drag={colDrag('left')}
    ondblclick={() => {
      leftW = LEFT_W;
      save('nebulosa.leftW', leftW);
    }}
  ></div>
  {/if}
  {#if rightOpen}
  <div
    class="vsplit"
    style="right: {rightW - 4}px"
    role="separator"
    aria-orientation="vertical"
    title="Drag to resize; double-click to reset"
    use:drag={colDrag('right')}
    ondblclick={() => {
      rightW = RIGHT_W;
      save('nebulosa.rightW', rightW);
    }}
  ></div>
  {/if}
  <header>
    <div class="brand">
      <button class="ghost side-btn" class:on={leftOpen} onclick={() => toggleSide('left')} title="Show / hide the settings ( [ )" aria-label="Settings panel">◧</button>
      <img class="logo light" src={instrixIcon} alt="INSTRIX" />
      <img class="logo dark" src={instrixIconWhite} alt="INSTRIX" />
      <strong>Nebulosa</strong>
      <span class="muted">Companion</span>
    </div>
    <div class="modes" role="tablist" aria-label="View" data-tour="modes">
      <button class:on={appMode === 'scope'} onclick={() => setAppMode('scope')} title="Waveforms: µs to seconds, triggers, markers">Scope</button>
      <button class:on={appMode === 'logger'} onclick={() => setAppMode('logger')} title="The whole session: minutes to days, logging to disk">Logger</button>
      <button class:on={appMode === 'sca'} onclick={() => setAppMode('sca')} title="Side-channel power analysis: capture traces, t-test, timing (ChipWhisperer-style). Beta — for slow targets.">Side-channel <span class="beta">beta</span></button>
    </div>
    <div class="status">
      {#if dev}
        <span class="dot ok"></span>{dev.link.description}
        <button class="ghost" onclick={disconnect}>Disconnect</button>
      {:else}
        {#if reconnecting}
          <span class="dot warn"></span><span class="warn">Reconnecting…</span>
          <button class="ghost" onclick={() => (reconnecting = false)}>Cancel</button>
        {:else}
          <span class="dot"></span><span class="muted">Not connected</span>
        {/if}
      {/if}
      <button class="ghost find" onclick={() => (paletteOpen = true)} title="Find a setting or action (Ctrl K)" aria-label="Find a setting or action">⌕ <kbd>Ctrl K</kbd></button>
      <button class="ghost" onclick={() => (showcasesOpen = true)} title="Tutorials and showcases: hands-on lessons and real measurements on the demo device" data-tour="showcases">Learn</button>
      <button class="ghost side-btn" onclick={() => (configOpen = true)} title="Configurations: save, load and apply setting profiles" aria-label="Configurations" data-tour="config">⚙</button>
      <button class="ghost side-btn" onclick={(e) => scaleMenu(e)} title="Appearance: UI scale (Ctrl + / −), theme" aria-label="UI scale"
        >Aa{#if canScale && uiScale.value !== 1}<small class="scale-pct">{Math.round(uiScale.value * 100)}%</small>{/if}</button
      >
      <button class="ghost side-btn" onclick={(e) => helpMenu(e)} title="Help, walkthrough, about (?)" aria-label="Help" data-tour="help">?</button>
      <button class="ghost side-btn" class:on={rightOpen} onclick={() => toggleSide('right')} title="Show / hide the meter ( ] )" aria-label="Meter panel">◨</button>
    </div>
  </header>

  <aside class:hidden={!leftOpen} data-tour="sidebar">
    {#if !dev}
      <ConnectPanel {candidates} {scanning} {busy} {recFiles} {recDir} onscan={scan} onconnect={connect} onpair={pair} onopen={openRecording} onopenfile={openFile} ondemo={startDemo} onshowcases={() => (showcasesOpen = true)} />
    {:else if playback}
      <section>
        <h2>Recording <button class="ghost small" onclick={disconnect}>Close</button></h2>
        <dl>
          <dt>File</dt><dd class="wrap">{dev.link.description.replace(/^Recording /, '')}</dd>
          {#if playback.created}<dt>Recorded</dt><dd>{new Date(String(playback.created)).toLocaleString()}</dd>{/if}
          {#if playback.link}<dt>From</dt><dd class="wrap">{playback.link}</dd>{/if}
          {#if playback.device}
            {@const d = playback.device as Info}
            <dt>Firmware</dt><dd>{d.fw_version_str}</dd>
            <dt>FPGA</dt><dd>{hex(d.fpga_id)}</dd>
          {/if}
          {#if playback.rate_hz}<dt>Rate</dt><dd>{fmtRate(Number(playback.rate_hz))}</dd>{/if}
          <dt>Length</dt><dd>{fmtTime(store.tEnd - store.tStart)}</dd>
          <dt>Samples</dt><dd>{(runMoments?.i.n ?? 0).toLocaleString()}</dd>
        </dl>
        <button onclick={fitAll}>Show all</button>
      </section>
    {:else if info}
      <!-- always in reach: start / stop, and what is streaming -->
      <div class="runbar">
        {#if streaming}
          <button class="danger" onclick={stop}>■ Stop</button>
        {:else}
          <button class="primary" onclick={start}>▶ Start</button>
        {/if}
        <span class="muted small">{SOURCE_NAME[source] ?? ''} · {fmtRate(rateHz)}</span>
      </div>

      <SectionNav items={navSections} />

      <Section id="device" icon="device" title="Device" tour="device" open={false} summary="{info.link_name} · firmware {info.fw_version_str}">
        <dl>
          <dt>Firmware</dt><dd>{info.fw_version_str} <small class="muted">{info.build}</small></dd>
          <dt>FPGA</dt>
          <dd>
            {hex(info.fpga_id)}
            {#if info.fpga_id < FPGA_ID_FORMAT5}<small class="warn">format 4: not decoded</small>{/if}
          </dd>
          <dt>Link</dt><dd>{info.link_name}</dd>
          <dt>Base rate</dt><dd>{fmtRate(info.base_rate_hz)}</dd>
          {#if info.ip}<dt>IP</dt><dd>{info.ip}</dd>{/if}
          {#if info.mac}<dt>MAC</dt><dd>{info.mac}</dd>{/if}
        </dl>
        {#if info.caps & CAP.BOARD}
          <button class="ghost small" onclick={() => (updating = true)} title="Firmware and gateware">Software update…</button>
        {/if}
      </Section>

      {#if info.caps & CAP.BOARD}
        <BoardPanel {dev} {streaming} bind:board onstop={stop} onstart={start} onnote={(m) => (note = m)} />
      {/if}

      <Section id="acquisition" icon="acquisition" title="Acquisition" tour="acquisition" summary="{SOURCE_NAME[source] ?? ''} · {fmtRate(rateHz)}{decimMode === 1 && log2n ? ' subsampled' : ''}">
        <label>
          Source
          <select bind:value={source} disabled={streaming}>
            <option value={SRC.STUB_RANDOM}>Stub: random</option>
            <option value={SRC.STUB_PATTERN}>Stub: pattern</option>
            <option value={SRC.AD7387}>ADC (AD7387)</option>
          </select>
        </label>
        {#if info.caps & CAP.DECIM}
          <label>
            Decimation N = {1 << log2n} <span class="muted">→ {fmtRate(rateHz)}</span>
            <input type="range" min="0" max="8" bind:value={log2n} onchange={() => run(applyDecimation)} />
          </label>
          <label>
            Mode
            <select bind:value={decimMode} onchange={() => run(applyDecimation)}>
              <option value={0}>Average</option>
              <option value={1}>Subsample</option>
            </select>
          </label>
        {/if}
        {#if info.caps & CAP.RANGE}
          <label class="check"><input type="checkbox" bind:checked={rangeAuto} disabled={streaming} /> Auto range</label>
          {#if rangeAuto}
            <div class="row">
              <label>Min <input type="number" min="0" max="5" bind:value={rangeMin} disabled={streaming} /></label>
              <label>Max <input type="number" min="0" max="5" bind:value={rangeMax} disabled={streaming} /></label>
            </div>
          {:else}
            <label>Range <input type="number" min="0" max="5" bind:value={rangeManual} disabled={streaming} /></label>
          {/if}
        {/if}
        {#if streaming}<small class="muted">Source and range change while stopped.</small>{/if}
      </Section>
    {/if}

    {#if appMode === 'logger'}
    <Section id="logger" icon="logger" title="Logger" tour="logger-panel" summary={logging?.active || exporting ? '● logging to file' : `${fmtTime(logInterval)} intervals`}>
      <label title="Per interval: min, max, mean and σ of every signal. In memory it stays bounded: older intervals merge 10×, then 100×.">
        Interval <span class="muted small">· ~{fmtBytesSize(sess.bytes)} in memory</span>
        <select value={logInterval} onchange={(e) => changeLogInterval(Number(e.currentTarget.value), e.currentTarget)}>
          {#each LOG_INTERVALS as iv (iv)}<option value={iv}>{fmtTime(iv)}</option>{/each}
        </select>
      </label>
      {#if logging?.active || exporting}
        <button class="danger" onclick={() => stopLog()}>■ Stop logging</button>
        <dl>
          <dt>File</dt><dd class="wrap">{logPath.split(/[\\/]/).pop()}</dd>
          {#if logging?.active}<dt>Entries</dt><dd>{logging.entries.toLocaleString()}</dd>{/if}
          <dt>Size</dt><dd>{fmtBytesSize(logging?.active ? logging.bytes : exportBytes)}</dd>
          {#if recording?.active}<dt>Raw</dt><dd>{fmtBytesSize(recording.bytes)}</dd>{/if}
          {#if logStarted}<dt>For</dt><dd>{fmtTime((nowTick - logStarted) / 1000)}</dd>{/if}
          {#if diskFree != null}<dt>Disk free</dt><dd>{fmtBytesSize(diskFree)}</dd>{/if}
        </dl>
      {/if}
      {#if isNative}
        {#if !logging?.active && !exporting}
          <div class="seg wide" data-tour="log-mode">
            <button class:on={out.cfg.data === 'summary'} onclick={() => setLogData('summary')} title="One row per interval: min, max, mean and σ of the samples in it">
              Per {fmtTime(logInterval)} (stats)
            </button>
            <button class:on={out.cfg.data === 'raw'} onclick={() => setLogData('raw')} title="Every sample at the full rate, as measured: no averaging">
              Every sample
            </button>
          </div>
          {#if out.cfg.data === 'raw'}
            <small class="muted">Every sample at {fmtRate(rateHz)}, no averaging: ~{fmtBytesSize(rawBytesPerS)}/s, {fmtBytesSize(rawBytesPerS * 3600)}/h.</small>
          {/if}
          <button class="primary" onclick={startLog} disabled={!dev || !!dev.link.playback || (out.kind !== 'native' && !out.cfg.fields.length)}>
            ● Start logging to file
          </button>
          <small class="muted">To ~/Documents/Nebulosa, as set in File output. Stops cleanly if the disk runs low.</small>
        {/if}
      {:else}
        <small class="muted">Files need the desktop app; File output below previews what would be written.</small>
      {/if}
      <button class="ghost" onclick={newSession} disabled={!sess.has_data}>New session</button>
    </Section>


    <Section id="output" icon="output" title="File output" open={false} summary="{out.kind === 'native' ? 'Nebulosa log' : out.cfg.format.type.toUpperCase()} · {out.cfg.data === 'raw' ? 'every sample' : 'per interval'}">
        <ExportEditor
          bind:out
          link={activeLink}
          {wallOffset}
          interval={logInterval}
          {rateHz}
          mathOn={mathInfo.on}
          disabled={!!logging?.active || exporting}
        />
        {#if out.kind === 'native' && out.cfg.data === 'raw'}
          <label>
            Raw recording limit
            <select bind:value={rawCapGB} disabled={!!logging?.active}>
              {#each [1, 5, 20, 100, 500] as g (g)}<option value={g}>{g} GB ({fmtTime((g * 1e9) / 4 / (rateHz || 2e6))} at {fmtRate(rateHz || 2e6)})</option>{/each}
            </select>
          </label>
        {/if}

    </Section>
    {:else}
      <TriggerPanel {trig} {available} onlevel50={level50} onarm={() => trig.rearm(store, view)} />
    {/if}

    <SignalsPanel {available} {signals} bind:shown bind:trace bind:axes bind:bufferMB bufferChoices={BUFFER_CHOICES} {bufferS} {rateHz} onbuffer={applyBuffer} />

    <MathPanel bind:draft={mathDraft} error={mathError} onapply={applyMath} onoff={mathOff} />

    <CalibrationPanel bind:cal ranges={CURRENT_RANGES} note={calNote} onapply={() => applyCal()} onreset={resetCal} />

    <Section id="monitor" icon="monitor" title="Serial monitor" tour="monitor" open={false} summary={monitor.open ? `● ${monitor.annotations.length} msg${monitor.matches ? ` · ${monitor.matches} hit` : ''}` : 'off'}>
      <p class="muted small">Read a serial port and mark its lines on the plots (Scope and Logger), timed to the measurements. Send, toggle RTS / DTR, trigger on a message. Saved as a <code>.serial.csv</code> next to a log.</p>
      {#if isNative && !monitor.open}
        <div class="mon-row">
          <select bind:value={monitorPort}>
            {#each monitorPorts as pt (pt.path)}<option value={pt.path}>{pt.label}</option>{/each}
          </select>
          <button class="ghost small" onclick={scanMonitorPorts}>Scan</button>
        </div>
      {/if}
      <div class="mon-grid">
        <label>Baud
          <select
            value={baudCustom || !BAUDS.includes(serialCfg.baud) ? 'custom' : String(serialCfg.baud)}
            onchange={(e) => {
              const v = (e.currentTarget as HTMLSelectElement).value;
              baudCustom = v === 'custom';
              if (!baudCustom) serialCfg.baud = Number(v);
            }}
            disabled={monitor.open}
          >
            {#each BAUDS as b (b)}<option value={String(b)}>{b.toLocaleString()}</option>{/each}
            <option value="custom">Custom…</option>
          </select>
          {#if baudCustom || !BAUDS.includes(serialCfg.baud)}
            <input type="number" min="50" step="1" bind:value={serialCfg.baud} disabled={monitor.open} title="Any rate the adapter supports" />
          {/if}
        </label>
        <label>Data <select bind:value={serialCfg.dataBits} disabled={monitor.open}>{#each [5, 6, 7, 8] as b (b)}<option value={b}>{b}</option>{/each}</select></label>
        <label>Parity <select bind:value={serialCfg.parity} disabled={monitor.open}><option value="none">none</option><option value="even">even</option><option value="odd">odd</option></select></label>
        <label>Stop <select bind:value={serialCfg.stopBits} disabled={monitor.open}><option value={1}>1</option><option value={2}>2</option></select></label>
      </div>
      <label>Flow control <select bind:value={serialCfg.flow} disabled={monitor.open}><option value="none">none</option><option value="hardware">hardware (RTS/CTS)</option>{#if isNative}<option value="software">software (XON/XOFF)</option>{/if}</select></label>
      <div class="mon-row">
        <label class="mon-chk"><input type="checkbox" bind:checked={serialCfg.rts} onchange={() => monitor.open && monitor.setSignals(serialCfg.rts, serialCfg.dtr)} /> RTS</label>
        <label class="mon-chk"><input type="checkbox" bind:checked={serialCfg.dtr} onchange={() => monitor.open && monitor.setSignals(serialCfg.rts, serialCfg.dtr)} /> DTR</label>
        <span class="muted small">{monitor.open ? 'toggles live' : 'asserted on open'}</span>
      </div>
      <button class="small" class:danger={monitor.open} class:primary={!monitor.open} onclick={monitorConnect}>
        {monitor.open ? '■ Disconnect' : isNative ? 'Open port' : 'Open port (Web Serial)'}
      </button>
      {#if monitor.error}<p class="error small">{monitor.error}</p>{/if}
      {#if monitor.open}
        <p class="mon-status"><span class="dot ok"></span>{monitor.handle?.name ?? 'open'} · ↓{fmtBytesSize(monitor.bytesIn)} ↑{fmtBytesSize(monitor.bytesOut)} · {monitor.annotations.length} msg</p>
      {/if}

      {#if monitor.open}
        <div class="mon-send">
          <input placeholder="Send… (↑ history)" bind:value={sendText} onkeydown={sendKey} />
          <select bind:value={serialCfg.txNewline} title="Line ending appended to what you send"><option value="none">—</option><option value="lf">\n</option><option value="cr">\r</option><option value="crlf">\r\n</option></select>
          <button class="small" onclick={monitorSend} disabled={!sendText}>Send</button>
        </div>
      {/if}

      <details class="mon-more">
        <summary>Line handling and trigger</summary>
        <label>Split received on <select bind:value={serialCfg.rxNewline}><option value="any">any (CR / LF / CRLF)</option><option value="lf">\n</option><option value="cr">\r</option><option value="crlf">\r\n</option></select></label>
        <label class="mon-chk"><input type="checkbox" bind:checked={serialCfg.echo} /> Echo sent lines on the log</label>
        <label class="mon-chk"><input type="checkbox" bind:checked={serialCfg.clearOnConnect} /> Clear the list when the port opens</label>
        <label class="mon-chk"><input type="checkbox" bind:checked={serialCfg.trigger.on} /> Trigger on a message</label>
        {#if serialCfg.trigger.on}
          <div class="mon-send">
            <input class="mono" placeholder={serialCfg.trigger.regex ? 'regex' : 'text to match'} bind:value={serialCfg.trigger.pattern} />
          </div>
          <label class="mon-chk"><input type="checkbox" bind:checked={serialCfg.trigger.regex} /> Regular expression</label>
          <label class="mon-chk"><input type="checkbox" bind:checked={serialCfg.trigger.hold} /> Freeze the view on a match (Scope: like a trigger)</label>
          <p class="muted small">A match is a red marker. {monitor.matches} so far.</p>
        {/if}
      </details>

      {#if monitor.open}
        <div class="mon-row">
          <button class="ghost small" class:on={monitor.paused} onclick={() => (monitor.paused = !monitor.paused)} title="Freeze the list; the port keeps receiving">{monitor.paused ? '▶ Resume' : '⏸ Pause'}</button>
          <span class="muted small">{monitor.annotations.length} msg{monitor.paused ? ' · frozen' : ''}</span>
        </div>
      {/if}
      {#if monitor.annotations.length}
        <div class="mon-list">
          {#each monitor.annotations.slice(-200).reverse() as a (a)}
            <div class="mon-msg" class:trig={a.kind === 'trigger'} class:tx={a.kind === 'tx'}>{#if serialCfg.timestamps}<button class="mon-jump" onclick={() => openInScope(a.t)} title="Open this moment in the scope">{new Date(a.wall ?? 0).toLocaleTimeString()}</button>{/if} <span>{monitorHex ? toHexLine(a.text) : a.text}</span></div>
          {/each}
        </div>
        <div class="mon-row">
          <label class="mon-chk"><input type="checkbox" bind:checked={monitorHex} /> Hex</label>
          <label class="mon-chk"><input type="checkbox" bind:checked={serialCfg.timestamps} /> Time</label>
          <span class="spacer"></span>
          <button class="ghost small" onclick={saveMessages}>Save…</button>
          <button class="ghost small" onclick={() => monitor.clear()}>Clear</button>
        </div>
      {/if}
    </Section>

    {#if error}
      <p class="error">{error}</p>
    {/if}
    {#if note}
      <p class="note small">
        {note}
        {#if noteAction}<button class="ghost small undo" onclick={() => { noteAction!.run(); note = ''; noteAction = null; }}>{noteAction.label}</button>{/if}
        <button class="ghost small" onclick={() => (noteHistoryOpen = !noteHistoryOpen)} title="Recent messages">🕐</button>
        <button class="ghost small" onclick={() => { note = ''; noteAction = null; }}>✕</button>
      </p>
      {#if noteHistoryOpen}
        <div class="note-history">
          {#each noteHistory.slice(-12).reverse() as h (h)}
            <div><span class="muted">{new Date(h.wall).toLocaleTimeString()}</span> {h.text}</div>
          {/each}
          {#if !noteHistory.length}<div class="muted">No messages yet.</div>{/if}
        </div>
      {/if}
    {/if}
  </aside>

  <main bind:this={mainEl}>
    {#if appMode === 'sca'}
      <div class="sca-wrap">
        <SideChannel
          bind:this={sca}
          link={activeLink}
          demo={dev && dev.link instanceof DemoLink ? dev.link.target : null}
          {trig}
          dt={1 / (rateHz || 2e6)}
          {signals}
          {streaming}
          onnote={(m) => (note = m)}
        />
      </div>
    {:else if appMode === 'logger'}
      <div class="logger-wrap" data-tour="logger">
        <LoggerView bind:this={logger} {store} session={sess} {signals} {axes} {trace} {wallOffset} bind:lv onopen={openInScope} intervals={LOG_INTERVALS} oninterval={changeLogInterval} onreset={resetView} annotations={monitor.annotations} onback={navBack} onforward={navForward} canBack={loggerHist.canBack} canForward={loggerHist.canForward} />
      </div>
      <!-- the same tab bar as under the Scope (the Logger has one list: the serial messages) -->
      <div class="markers-wrap logger-msgs" class:folded={!loggerMsgsOpen} data-tour="logger-msgs" style={loggerMsgsOpen ? 'height: min(240px, 26vh)' : ''}>
        <div class="tabs">
          <button class="on" onclick={() => (loggerMsgsOpen = true)}>Serial <small>{monitor.annotations.length.toLocaleString()}</small></button>
          <span class="sp"></span>
          <button class="fold" onclick={() => (loggerMsgsOpen = !loggerMsgsOpen)} aria-expanded={loggerMsgsOpen} title={loggerMsgsOpen ? 'Fold the list' : 'Show the list'}>{loggerMsgsOpen ? '▾' : '▸'}</button>
        </div>
        {#if loggerMsgsOpen}
          <SerialList messages={monitor.annotations} wallClock onjump={jumpLoggerTo} onclear={() => monitor.clear()} onsave={saveMessages} />
        {/if}
      </div>
    {:else}
    <div class="strip" data-tour="strip">
      <span class="badge" class:run={streaming} class:stop={!streaming}>{streaming ? 'RUN' : 'STOP'}</span>
      <button
        class="badge btn"
        class:live={view.live}
        class:hold={!view.live}
        onclick={toggleLive}
        title="Follow the newest data (Space)">{view.live ? 'LIVE' : 'HOLD'}</button
      >
      {#if trig.mode !== 'roll'}
        <span class="badge trig {trig.status}" title="Trigger: {trig.mode}">
          {({ roll: 'ROLL', wait: 'WAIT', trigd: "TRIG'D", auto: 'AUTO', stop: 'STOP' } as const)[trig.status]}
        </span>
      {/if}
      <button class="small autoset" onclick={autoset} disabled={!hasData} title="Trigger, time/div and axes from the signal (like a scope's Autoset)">Autoset</button>
      <button class="small settrig" class:on={pickTrigger} onclick={() => (pickTrigger = !pickTrigger)} disabled={!hasData} title="Set the trigger source and level by clicking a point on a plot">⌖ {pickTrigger ? 'Click a plot…' : 'Set trigger'}</button>
      <button class="small analyze" onclick={openAnalysis} disabled={!hasData} title="Histogram, CDF, spectrum and worst window over the active dual marker, else the screen (A)"
        >Analyze{view.activeMarker?.t1 != null ? ' marker' : ''}…</button
      >
      <span class="navbtns">
        <button class="ghost small" onclick={navBack} disabled={!scopeHist.canBack} title="Back to the view before (Alt ←, the mouse's back button)" aria-label="Back">◀</button>
        <button class="ghost small" onclick={navForward} disabled={!scopeHist.canForward} title="Forward again (Alt →)" aria-label="Forward">▶</button>
      </span>
      <button class="ghost small" onclick={resetView} title="Plots back as they first were: equal heights, axes on auto, mean and min–max, 100 ms/div, live">Reset view</button>
      {#if autosetNote}<span class="autoset-note" role="status">{autosetNote}</span>{/if}
      <div class="tdiv">
        <button class="ghost small" onclick={() => view.zoom(store, -1)} disabled={view.divIndex === 0} title="Faster (+, wheel)">−</button>
        <span class="mono">{fmtTime(view.timeDiv)}/div</span>
        <button
          class="ghost small"
          onclick={() => view.zoom(store, 1)}
          disabled={view.divIndex === TIME_DIVS.length - 1 || TIME_DIVS[view.divIndex + 1] * 10 > Math.max(bufferS, 1)}
          title="Slower (-, wheel)">+</button
        >
      </div>
      {#if rateHz}<span class="mono muted">{fmtRate(rateHz)} · N={1 << log2n}</span>{/if}
      <div class="tdiv">
        <button class="ghost small" onclick={addSingleMarker} disabled={!hasData} title="Single marker (M, or double-click a plot)">+ Marker</button>
        <button class="ghost small" onclick={addDualMarker} disabled={!hasData} title="Dual marker (D, or drag across a plot)">+ Dual</button>
        {#if view.markers.length}
          <button class="ghost small" onclick={clearMarkersUndoable} title="Remove all markers">Clear</button>
        {/if}
      </div>
      <div class="tdiv">
        {#if isNative && dev && !dev.link.playback}
          <button class="small rec" class:on={recording?.active} onclick={toggleRecording} title="Record every sample to a file">
            {#if recording?.active}■ Stop · {fmtTime(recording.seconds)} · {fmtBytesSize(recording.bytes)}{:else}● REC{/if}
          </button>
        {/if}
        {#if isNative}
          <button class="ghost small" onclick={exportCsv} disabled={!hasData} title="Every sample of the active dual marker's span (or the screen) as CSV">CSV</button>
        {/if}
        <button class="ghost small" onclick={exportPng} disabled={!hasData} title="The plots as a PNG image">PNG</button>
      </div>
      <span class="legend muted" title="Each pixel column covers many samples">
        {#if trace.mean}<span class="key"><i class="line"></i>mean</span>{/if}
        {#if trace.range}<span class="key"><i class="band"></i>min–max</span>{/if}
        {#if trace.sigma}<span class="key"><i class="sd"></i>±σ</span>{/if}
        <span class="key"><i class="gap"></i>gap</span>
      </span>
      <button class="hint muted linkish" onclick={() => (helpOpen = true)} title="All the keys and mouse gestures (?)"
        >wheel: time/div · shift+wheel: pan · ctrl+drag: zoom in · space: live/hold · right-click: menu · ? keys</button
      >
    </div>
    {#if frontEndOff}
      <div class="banner warn">
        <b>Analog front end off</b>
        <span>{board?.apwr_ok ? 'Its supplies are off: the ADC reads nothing real.' : 'It needs 12 V: the barrel jack, or a USB-C charger set to 12 V (Board panel).'}</span>
        {#if board?.apwr_ok}<button class="small primary" onclick={frontEndOn}>Turn on</button>{/if}
      </div>
    {/if}
    <div class="overview-wrap" data-tour="overview">
      <Overview {store} {view} />
    </div>
    <div class="scope-wrap" data-tour="plots">
      <Scope bind:this={scope} {store} {view} {signals} {axes} {trace} {trig} onmenu={scopeMenu} picking={pickTrigger} onpick={onPickTrigger} annotations={monitor.annotations} />
    </div>
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="splitter"
      role="separator"
      aria-orientation="horizontal"
      title="Drag to resize; double-click to reset"
      onpointerdown={startSplit}
      ondblclick={() => (listH = 190)}
    ></div>
    <div class="markers-wrap" data-tour="lists" style="height: min({listH}px, 22vh)">
      <div class="tabs">
        <button class:on={bottomTab === 'markers'} onclick={() => (bottomTab = 'markers')}>Markers <small>{view.markers.length}</small></button>
        <button class:on={bottomTab === 'events'} onclick={() => (bottomTab = 'events')}>Events <small>{trig.count.toLocaleString()}</small></button>
        <button class:on={bottomTab === 'serial'} onclick={() => (bottomTab = 'serial')}>Serial <small>{monitor.annotations.length.toLocaleString()}</small></button>
      </div>
      {#if bottomTab === 'serial'}
        <SerialList
          messages={monitor.annotations}
          origin={scopeOrigin}
          onjump={jumpToSerial}
          onclear={() => monitor.clear()}
          onsave={saveMessages}
        />
      {:else if bottomTab === 'events'}
        <EventList
          {trig}
          unit={SIGNAL_INFO[trig.source].unit}
          integralUnit={SIGNAL_INFO[trig.source].integral ?? ''}
          origin={trig.mode !== 'roll' && trig.shown != null ? trig.shown : store.tEnd}
          onjump={jumpToEvent}
          onclear={() => trig.clearLog(activeLink)}
          onexport={exportEvents}
          onmenu={eventMenu}
        />
      {:else}
      <MarkerList
        math={mathInfo.on}
        rows={markerRows}
        active={view.active}
        origin={trig.mode !== 'roll' && trig.shown != null ? trig.shown : store.tEnd}
        onpick={(id) => (view.active = id)}
        ondelete={(id) => view.remove(id)}
        onzoom={zoomToMarker}
        onanalyze={analyzeMarker}
        onclear={clearMarkersUndoable}
        onmenu={markerListMenu}
      />
      {/if}
    </div>
    {/if}
    <footer>
      {#if stats}
        <span>{fmtBytes(throughput)}</span>
        <span>blocks {stats.blocks}</span>
        <span>samples {stats.stream.samples}</span>
        <span class:bad={stats.lost_blocks}>lost {stats.lost_blocks}</span>
        <span class:bad={stats.stall_flags}>stalls {stats.stall_flags}</span>
        <span class:bad={stats.stream.ovf_blocks}>OVF {stats.stream.ovf_blocks}</span>
        <span class:bad={stats.stream.seq_errors}>seq err {stats.stream.seq_errors}</span>
        <span class:bad={stats.stream.malformed}>malformed {stats.stream.malformed}</span>
        <span>dirty {stats.stream.dirty}</span>
        <span>range Δ {stats.stream.range_changes}</span>
      {:else}
        <span class="muted">No stream.</span>
      {/if}
    </footer>
  </main>

  <div class="meter-col" class:hidden={!rightOpen} data-tour="meter">
    <Meter
      {summary}
      accum={accumView}
      {signals}
      bind:scope={meterScope}
      bind:hold
      hasSelection={!!view.selection}
      onclear={clearAccum}
      onanalyze={hasData ? openAnalysis : undefined}
    />
  </div>
</div>

<style>
  .vsplit {
    position: absolute;
    top: 48px;
    bottom: 0;
    width: 7px;
    cursor: col-resize;
    touch-action: none;
    z-index: 5;
  }
  .vsplit:hover,
  .vsplit:active {
    background: linear-gradient(90deg, transparent 2px, var(--accent) 2px, var(--accent) 4px, transparent 4px);
  }
  .app {
    position: relative;
    display: grid;
    grid-template-rows: 48px 1fr;
    height: 100vh;
  }
  header {
    grid-column: 1 / -1;
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 16px;
    border-bottom: 1px solid var(--border);
    background: var(--panel);
  }
  .modes .beta {
    font-size: 9px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    padding: 1px 4px;
    margin-left: 4px;
    border-radius: 999px;
    background: var(--warn, #e0a030);
    color: #1a1205;
    vertical-align: middle;
  }
  .linkish {
    background: none;
    border: 0;
    padding: 0;
    font: inherit;
    cursor: pointer;
    text-align: left;
  }
  .linkish:hover {
    color: var(--text, inherit);
    text-decoration: underline;
  }
  .hidden {
    display: none !important;
  }
  .side-btn {
    padding: 2px 8px;
    font-size: 16px;
    line-height: 1;
    color: var(--muted);
  }
  .scale-pct {
    font-size: 10px;
    margin-left: 3px;
  }
  .side-btn.on {
    color: var(--accent);
  }
  .brand {
    display: flex;
    gap: 8px;
    align-items: center;
  }
  .logo {
    height: 18px;
    width: auto;
  }
  .logo.dark {
    display: none;
  }
  :global(:root[data-theme='dark']) .logo.light {
    display: none;
  }
  :global(:root[data-theme='dark']) .logo.dark {
    display: block;
  }
  .status {
    display: flex;
    gap: 10px;
    align-items: center;
    font-size: 13px;
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: var(--r-dot);
    background: var(--muted);
  }
  .dot.ok {
    background: var(--ok);
  }
  .dot.warn {
    background: var(--warn);
  }
  .status .warn {
    color: var(--warn);
  }
  aside {
    grid-column: 1;
    grid-row: 2;
    border-right: 1px solid var(--border);
    background: var(--panel);
    overflow-y: auto;
    padding: 8px 16px 16px;
  }
  .meter-col {
    grid-column: 3;
    grid-row: 2;
    border-left: 1px solid var(--border);
    background: var(--bg);
    overflow-y: auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  main {
    grid-column: 2;
    grid-row: 2;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }
  .modes {
    display: inline-flex;
    border: 1px solid var(--border);
    border-radius: var(--r);
    overflow: hidden;
  }
  .modes button {
    border: 0;
    border-radius: 0;
    background: transparent;
    padding: 4px 16px;
    font-weight: 600;
  }
  .modes button.on {
    background: var(--accent);
    color: #fff;
  }
  .sca-wrap {
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }
  .logger-wrap {
    flex: 1;
    min-height: 0;
    padding: 8px 12px 12px;
  }
  .find {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .find kbd {
    font: 10px ui-monospace, monospace;
    padding: 0 5px;
    border: 1px solid var(--border);
    border-radius: 4px;
    color: var(--muted);
  }
  .tabs .sp {
    flex: 1;
  }
  .tabs button.fold {
    border: 0;
    background: none;
    padding: 3px 8px;
    color: var(--muted);
  }
  .logger-msgs.folded .tabs button.on {
    border-radius: var(--r);
    border-bottom: 1px solid var(--border);
  }
  .navbtns {
    display: inline-flex;
    gap: 2px;
  }
  .strip {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 12px 0;
    font-size: 13px;
    flex-wrap: wrap;
  }
  .badge {
    font: 600 11px ui-monospace, monospace;
    letter-spacing: 0.08em;
    padding: 3px 8px;
    border-radius: var(--r-sm);
    border: 1px solid transparent;
  }
  .badge.run {
    background: var(--live);
    color: #000;
  }
  .badge.stop {
    background: var(--danger);
    color: #fff;
  }
  .badge.btn.live {
    border-color: var(--live);
    color: var(--live);
    background: transparent;
  }
  .badge.btn.hold {
    background: var(--hold);
    border-color: var(--hold);
    color: #000;
  }
  .badge.trig {
    border-color: var(--muted);
    color: var(--muted);
  }
  .badge.trig.trigd {
    background: var(--live);
    border-color: var(--live);
    color: #000;
  }
  .badge.trig.wait {
    border-color: var(--hold);
    color: var(--hold);
  }
  .badge.trig.auto {
    border-color: var(--accent);
    color: var(--accent);
  }
  .badge.trig.stop {
    background: var(--danger);
    border-color: var(--danger);
    color: #fff;
  }
  .autoset-note {
    font-size: 12px;
    padding: 2px 8px;
    border: 1px solid var(--accent);
    border-radius: var(--r-sm);
    color: var(--text);
    background: color-mix(in srgb, var(--accent) 15%, transparent);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 420px;
  }
  .settrig.on {
    border-color: var(--accent);
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 16%, transparent);
  }
  .autoset,
  .analyze {
    font-weight: 600;
    border-color: var(--accent);
    color: var(--accent);
    background: transparent;
  }
  .rec {
    color: var(--danger);
    border-color: var(--danger);
    background: transparent;
    font-weight: 600;
  }
  .rec.on {
    background: var(--danger);
    color: #fff;
    animation: blink 1.6s ease-in-out infinite;
  }
  @keyframes blink {
    50% {
      opacity: 0.75;
    }
  }
  .tdiv {
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }
  .legend {
    margin-left: auto;
    display: flex;
    gap: 10px;
    font-size: 11px;
  }
  .key {
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }
  .key i {
    display: inline-block;
    width: 14px;
  }
  .key .line {
    height: 2px;
    background: var(--muted);
  }
  .key .band {
    height: 9px;
    background: color-mix(in srgb, var(--muted) 35%, transparent);
  }
  .key .sd {
    height: 5px;
    background: color-mix(in srgb, var(--muted) 60%, transparent);
  }
  .key .gap {
    width: 0;
    height: 10px;
    border-left: 2px dashed var(--danger);
  }
  .hint {
    font-size: 11px;
    cursor: help;
  }
  .banner {
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 0 12px 6px;
    padding: 6px 10px;
    border-radius: var(--r);
    font-size: 13px;
    border: 1px solid var(--warn);
    background: color-mix(in srgb, var(--warn) 12%, transparent);
  }
  .banner b {
    color: var(--warn);
    white-space: nowrap;
  }
  .banner button {
    margin-left: auto;
  }
  .overview-wrap {
    padding: 8px 12px 0;
  }
  .scope-wrap {
    flex: 1;
    min-height: 0;
    padding: 8px 12px 8px;
  }
  .markers-wrap {
    padding: 0 12px 10px;
    display: flex;
    flex-direction: column;
    flex: none;
    min-height: 0;
  }
  .markers-wrap > :global(:last-child) {
    flex: 1;
    min-height: 0;
  }
  .splitter {
    flex: none;
    height: 8px;
    margin: -2px 12px 2px;
    cursor: row-resize;
    touch-action: none;
    position: relative;
  }
  .splitter::after {
    content: '';
    position: absolute;
    left: 50%;
    top: 3px;
    width: 40px;
    height: 3px;
    margin-left: -20px;
    border-radius: var(--r-sm);
    background: var(--border);
  }
  .splitter:hover::after,
  .splitter:active::after {
    background: var(--accent);
    width: 100%;
    left: 0;
    margin-left: 0;
  }
  .tabs {
    display: flex;
    gap: 2px;
    margin-bottom: -1px;
  }
  .tabs button {
    border: 1px solid var(--border);
    border-bottom: 0;
    border-radius: var(--r) var(--r) 0 0;
    background: var(--bg);
    padding: 3px 12px;
    font-size: 12px;
  }
  .tabs button.on {
    background: var(--panel);
    font-weight: 600;
  }
  .tabs small {
    color: var(--muted);
    margin-left: 4px;
  }
  footer {
    display: flex;
    flex-wrap: nowrap;
    overflow: hidden;
    white-space: nowrap;
    gap: 16px;
    padding: 8px 16px;
    border-top: 1px solid var(--border);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    color: var(--muted);
  }
  .bad {
    color: var(--danger);
  }
  .row {
    display: flex;
    gap: 8px;
  }
  .row > * {
    flex: 1;
  }
  .wrap {
    word-break: break-all;
  }
  .mon-row {
    display: flex;
    gap: 6px;
    align-items: center;
  }
  .mon-row .spacer {
    flex: 1;
  }
  .mon-status {
    display: flex;
    align-items: center;
    gap: 5px;
    margin: 0;
    font: 11px ui-monospace, monospace;
    color: var(--muted);
  }
  .mon-status .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
  }
  .mon-row select {
    flex: 1;
    min-width: 0;
  }
  .mon-baud {
    display: flex;
    flex-direction: column;
    gap: 3px;
    font-size: 12px;
    color: var(--muted);
  }
.mon-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 6px;
  }
  .mon-grid label,
  .mon-baud {
    display: flex;
    flex-direction: column;
    gap: 3px;
    font-size: 12px;
    color: var(--muted);
  }
  .mon-chk {
    display: flex;
    flex-direction: row;
    align-items: center;
    gap: 5px;
    font-size: 12px;
    color: var(--muted);
  }
  .mon-chk input {
    width: auto;
  }
  .mon-send {
    display: flex;
    gap: 6px;
  }
  .mon-send input {
    flex: 1;
    min-width: 0;
  }
  .mon-send select {
    width: auto;
    flex: none;
  }
  .mon-more summary {
    font-size: 12px;
    color: var(--muted);
    cursor: pointer;
    padding: 2px 0;
  }
  .mon-more {
    display: flex;
    flex-direction: column;
  }
  .mon-more[open] {
    gap: 7px;
  }
  .mon-list {
    max-height: 160px;
    overflow-y: auto;
    border: 1px solid var(--border);
    border-radius: var(--r-sm);
    background: var(--bg);
    font: 11px ui-monospace, monospace;
    padding: 3px;
  }
  .mon-msg {
    display: flex;
    gap: 6px;
    padding: 1px 2px;
    white-space: nowrap;
  }
  .mon-msg span {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .mon-jump {
    border: 0;
    background: none;
    color: var(--accent);
    cursor: pointer;
    padding: 0;
    font: inherit;
    flex: none;
  }
  .mon-msg.trig {
    color: var(--danger);
    font-weight: 600;
  }
  .mon-msg.tx span {
    color: var(--muted);
  }
  .note-history {
    margin: 2px 0 6px;
    padding: 6px 8px;
    border: 1px solid var(--border);
    border-radius: var(--r-sm);
    background: var(--bg);
    font-size: 11px;
    display: flex;
    flex-direction: column;
    gap: 2px;
    max-height: 160px;
    overflow-y: auto;
  }
  .note .undo {
    color: var(--accent);
  }
  .note {
    color: var(--ok);
    word-break: break-word;
  }
  .mono {
    font-family: ui-monospace, monospace;
    font-variant-numeric: tabular-nums;
  }
  .error {
    color: var(--danger);
    font-size: 13px;
    word-break: break-word;
  }
  .warn {
    color: var(--warn);
  }
  .small {
    font-size: 12px;
  }
  .muted {
    color: var(--muted);
  }
  .runbar {
    position: sticky;
    top: -8px;
    z-index: 4;
    margin: -8px -16px 0;
    padding: 10px 16px;
    background: var(--panel);
    border-bottom: 1px solid var(--border);
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .runbar button {
    flex: none;
    min-width: 96px;
  }
  /* the sidebar's panels (components too): shared look */
  aside :global {
    section {
      padding: 12px 0;
      border-bottom: 1px solid var(--border);
      display: flex;
      flex-direction: column;
      gap: 10px;
    }
    h2 {
      font-size: 12px;
      text-transform: uppercase;
      letter-spacing: 0.06em;
      color: var(--muted);
      margin: 0;
      display: flex;
      justify-content: space-between;
      align-items: center;
    }
    details summary {
      cursor: pointer;
      list-style: none;
    }
    details summary h2 {
      display: inline;
    }
    details summary::before {
      content: '▸ ';
      color: var(--muted);
    }
    details[open] summary::before {
      content: '▾ ';
    }
    .cal {
      display: flex;
      flex-direction: column;
      gap: 8px;
      margin-top: 8px;
    }
    .device {
      display: flex;
      flex-direction: column;
      align-items: flex-start;
      gap: 2px;
      text-align: left;
    }
    .device small {
      color: var(--muted);
    }
    dl {
      display: grid;
      grid-template-columns: auto 1fr;
      gap: 4px 12px;
      margin: 0;
      font-size: 13px;
    }
    dt {
      color: var(--muted);
    }
    dd {
      margin: 0;
      font-variant-numeric: tabular-nums;
    }
    label {
      display: flex;
      flex-direction: column;
      gap: 4px;
      font-size: 13px;
    }
    label.check {
      flex-direction: row;
      align-items: center;
    }
    label.inline {
      font-size: 12px;
    }
    .chips {
      display: flex;
      gap: 6px;
      flex-wrap: wrap;
    }
    .chip {
      flex-direction: row;
      align-items: center;
      gap: 4px;
      padding: 3px 8px;
      border: 1px solid var(--border);
      border-left: 3px solid var(--c);
      border-radius: var(--r);
      font-size: 12px;
    }
    .row {
      display: flex;
      gap: 8px;
    }
    .row > * {
      flex: 1;
    }
    details.out summary {
      cursor: pointer;
      list-style: none;
      margin-bottom: 6px;
    }
    details.out summary h2 {
      display: inline;
    }
    details.out summary::before {
      content: '▸ ';
      color: var(--muted);
    }
    details.out[open] summary::before {
      content: '▾ ';
    }
    .seg.wide {
      display: flex;
    }
    .seg.wide button {
      flex: 1;
    }
    .recs {
      display: flex;
      flex-direction: column;
      gap: 6px;
      max-height: 260px;
      overflow-y: auto;
    }
    .path {
      font-size: 11px;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }
    .wrap {
      word-break: break-all;
    }
    .note {
      color: var(--ok);
      word-break: break-word;
    }
    .fifty {
      flex: none !important;
      align-self: flex-end;
      margin-bottom: 2px;
    }
    .mono {
      font-family: ui-monospace, monospace;
      font-variant-numeric: tabular-nums;
    }
    details.more summary {
      cursor: pointer;
      font-size: 12px;
      color: var(--muted);
      margin-bottom: 6px;
    }
    .axis {
      display: flex;
      justify-content: space-between;
      align-items: center;
      font-size: 13px;
    }
    .seg {
      display: inline-flex;
      border: 1px solid var(--border);
      border-radius: var(--r);
      overflow: hidden;
    }
    .seg button {
      border: 0;
      border-radius: 0;
      background: transparent;
      padding: 3px 10px;
      font-size: 12px;
    }
    .seg button.on {
      background: var(--accent);
      color: #fff;
    }
    .seg.wide {
      display: flex;
    }
    .seg.wide button {
      flex: 1;
    }
    .error {
      color: var(--danger);
      font-size: 13px;
      word-break: break-word;
    }
    .warn {
      color: var(--warn);
    }
    .small {
      font-size: 12px;
    }
    .muted {
      color: var(--muted);
    }
  }
</style>
