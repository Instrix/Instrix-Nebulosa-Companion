<script lang="ts">
  import { onMount, untrack } from 'svelte';
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
  import Help from './lib/Help.svelte';
  import * as boardCmd from './lib/board';
  import LoggerView, { type LogView } from './lib/LoggerView.svelte';
  import ExportEditor, { defaultOutput, type OutputCfg } from './lib/ExportEditor.svelte';
  import { drag, stored, store as save } from './lib/drag';

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
  let source = $state<number>(SRC.STUB_RANDOM);
  let log2n = $state(4);
  let decimMode = $state(0);
  let rateHz = $state(0);
  let rangeAuto = $state(true);
  let rangeManual = $state(0);
  let rangeMin = $state(0);
  let rangeMax = $state(5);

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
  let axes = $state<Record<Signal, AxisCfg>>({
    i: { auto: true, min: 0, max: 0.1, log: false },
    v: { auto: true, min: 0, max: 5, log: false },
    p: { auto: true, min: 0, max: 0.5, log: false },
    m: { auto: true, min: 0, max: 1, log: false },
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
  /** a recording open for playback: its header */
  let playback = $state<Record<string, unknown> | null>(null);
  let fitPending = false;
  let scope = $state<Scope>();

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
      if (n > 5e6 && !confirm(`${(n / 1e6).toFixed(1)} M samples, about ${fmtBytesSize(n * 60)} of CSV. Export?`)) return;
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

  let bottomTab = $state<'markers' | 'events'>('markers');
  let updating = $state(false);
  let helpOpen = $state(false);

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
    const st = await store.stats(t0, t1);
    // the most active: largest swing for its level (power follows current)
    const score = (s: Signal) => {
      const m = st[s];
      return m.n ? m.pp / Math.max(Math.abs(m.mean), m.pp * 0.01, 1e-12) : 0;
    };
    const src: Signal = score('v') > 1.5 * score('i') ? 'v' : 'i';
    const m = st[src];
    for (const s of SIGNALS) axes[s].auto = true;
    if (!m.n || m.pp < Math.abs(m.mean) * 0.02) {
      // flat: nothing to trigger on
      trig.mode = 'roll';
      view.divIndex = TIME_DIVS.indexOf(0.1);
      view.live = true;
      note = 'Autoset: no activity to trigger on; rolling.';
      return;
    }
    const level = (m.min + m.max) / 2;
    const hyst = (m.max - m.min) * 0.1;

    /** median interval between rising crossings over [a, b] */
    async function period(a: number, b: number): Promise<number | null> {
      const L = await store.lod(a, b, 8000);
      const c = L.ch[src];
      let armed = false;
      const ups: number[] = [];
      for (let k = 0; k < L.t.length; k++) {
        const lo = c.min[k], hi = c.max[k];
        if (lo == null || hi == null) {
          armed = false; // a gap
          continue;
        }
        // armed by a column wholly below (a column holding a falling edge has
        // lows and highs too: it mustn't count as a rising one)
        if (armed && hi >= level) {
          ups.push(L.t[k]);
          armed = false;
        } else if (hi < level - hyst) armed = true;
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
    note = `Autoset: ${SIGNAL_INFO[src].name.toLowerCase()} ↑ at ${fmtSI(level, SIGNAL_INFO[src].unit, 3)}, period ${p >= 4 ? '–' : fmtTime(p)}, ${fmtTime(TIME_DIVS[k])}/div`;
  }

  // ---- scope / logger ------------------------------------------------------------
  let appMode = $state<'scope' | 'logger'>(stored('nebulosa.modeIsLogger', 0) === 1 ? 'logger' : 'scope');
  function setAppMode(m: 'scope' | 'logger') {
    appMode = m;
    save('nebulosa.modeIsLogger', m === 'logger' ? 1 : 0);
  }

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

  const LOG_INTERVALS = [0.01, 0.1, 1, 10];
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

  async function changeLogInterval(iv: number) {
    if (store.session.has_data && !confirm('A new interval starts the session log over. Go on?')) return;
    logInterval = iv;
    save('nebulosa.logInterval', iv);
    await activeLink?.setLogInterval(iv);
  }

  async function newSession() {
    if (!confirm('Clear the session log and start over?')) return;
    await activeLink?.clearSession();
    lv = { follow: true, last: 0, t0: 0, t1: 0 };
  }

  async function startLog() {
    if (out.kind !== 'native') return startExportLog();
    error = '';
    try {
      save('nebulosa.rawCapGB', rawCapGB);
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
  let analysis = $state<{ span: [number, number]; label: string } | null>(null);

  function openAnalysis() {
    const m = view.activeMarker;
    if (m && m.t1 != null) analysis = { span: [m.t0, m.t1], label: `${markerName(m)}: ${fmtTime(m.t1 - m.t0)}` };
    else {
      const sp = view.span(store);
      analysis = { span: sp, label: `the screen: ${fmtTime(sp[1] - sp[0])}` };
    }
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

  async function refreshMeter() {
    if (hold || meterBusy) return;
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
      accumView = accum
        ? {
            charge: accum.charge - accumBase.charge,
            energy: accum.energy - accumBase.energy,
            seconds: accum.seconds - accumBase.seconds,
          }
        : null;
    } catch (e) {
      console.warn('meter', e);
    } finally {
      meterBusy = false;
    }
  }

  function clearAccum() {
    if (accum) accumBase = { ...accum };
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

  function onKey(e: KeyboardEvent) {
    const tag = (e.target as HTMLElement).tagName;
    if (tag === 'INPUT' || tag === 'SELECT' || tag === 'TEXTAREA') return;
    if (e.key === ' ') {
      e.preventDefault();
      toggleLive();
    } else if (e.key === 'Escape') view.active = null;
    else if (e.key === 'Delete' || e.key === 'Backspace') view.remove(view.active);
    else if (e.key === 'm') addSingleMarker();
    else if (e.key === 'd') addDualMarker();
    else if (e.key === '+' || e.key === '=') view.zoom(store, -1);
    else if (e.key === '-') view.zoom(store, 1);
    else if (e.key === 'ArrowLeft') view.pan(store, -0.1);
    else if (e.key === 'ArrowRight') view.pan(store, 0.1);
    else if (e.key === '?' || e.key === 'F1') {
      e.preventDefault();
      helpOpen = true;
    } else if (e.key === '[') toggleSide('left');
    else if (e.key === ']') toggleSide('right');
  }

  /** fits the screen to a marker: a dual one fills ~80 % of it, a single one is centred */
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
      ...(dual
        ? [
            { label: 'Analyze…', run: () => analyzeMarker(id) },
            ...(isNative ? [{ label: 'Export samples to CSV…', run: () => exportMarker(id) }] : []),
          ]
        : []),
      { label: 'Delete', hint: 'Del', danger: true, run: () => view.remove(id) },
    ];
  }

  function allMarkerItems(): MenuEntry[] {
    const n = view.markers.length;
    const duals = view.markers.filter((m) => m.t1 != null).length;
    return [
      { label: `Delete all markers${n ? ` (${n})` : ''}`, danger: true, disabled: !n, run: () => view.clearMarkers() },
      ...(duals && duals < n
        ? [
            { label: `Delete the dual markers (${duals})`, danger: true, run: () => view.markers.filter((m) => m.t1 != null).forEach((m) => view.remove(m.id)) },
            { label: `Delete the single markers (${n - duals})`, danger: true, run: () => view.markers.filter((m) => m.t1 == null).forEach((m) => view.remove(m.id)) },
          ]
        : []),
    ];
  }

  /** puts the trigger on `sig` at `v` (a window: the same width, centred on v) */
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
    reset();
    if (d) await d.link.close().catch(() => undefined);
  }

  function onMeta(m: Meta) {
    store.update(m);
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
    scan();
    refreshRecordings();
    const t = setInterval(refreshMeter, 250);
    const wd = setInterval(watchdog, 2000);
    if (demoRequested) {
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
      dev?.link.close();
    };
  });
</script>

<svelte:window onkeydown={onKey} />
<ContextMenu />

{#if updating && dev}
  <Updater {dev} onclose={() => (updating = false)} onrestart={() => { updating = false; boardRestarts(); }} />
{/if}

{#if helpOpen}
  <Help onclose={() => (helpOpen = false)} />
{/if}

{#if analysis}
  <Analysis signals={available} link={activeLink} span={analysis.span} spanLabel={analysis.label} onclose={() => (analysis = null)} onshow={showSpan} />
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
      <span class="logo">◉</span>
      <strong>Nebulosa</strong>
      <span class="muted">Companion</span>
    </div>
    <div class="modes" role="tablist" aria-label="View">
      <button class:on={appMode === 'scope'} onclick={() => setAppMode('scope')} title="Waveforms: µs to seconds, triggers, markers">Scope</button>
      <button class:on={appMode === 'logger'} onclick={() => setAppMode('logger')} title="The whole session: minutes to days, logging to disk">Logger</button>
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
      <button class="ghost side-btn" onclick={() => (helpOpen = true)} title="Keys and mouse (?)" aria-label="Help">?</button>
      <button class="ghost side-btn" class:on={rightOpen} onclick={() => toggleSide('right')} title="Show / hide the meter ( ] )" aria-label="Meter panel">◨</button>
    </div>
  </header>

  <aside class:hidden={!leftOpen}>
    {#if !dev}
      <ConnectPanel {candidates} {scanning} {busy} {recFiles} {recDir} onscan={scan} onconnect={connect} onpair={pair} onopen={openRecording} onopenfile={openFile} ondemo={startDemo} />
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
      <section>
        <h2>Device</h2>
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
      </section>

      {#if info.caps & CAP.BOARD}
        <BoardPanel {dev} {streaming} bind:board onstop={stop} onnote={(m) => (note = m)} />
      {/if}

      <section>
        <h2>Acquisition</h2>
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
        <div class="row">
          {#if streaming}
            <button class="danger" onclick={stop}>Stop</button>
          {:else}
            <button class="primary" onclick={start}>Start</button>
          {/if}
        </div>
      </section>
    {/if}

    {#if appMode === 'logger'}
    <section>
      <h2>Logger</h2>
      <label>
        Interval
        <select value={logInterval} onchange={(e) => changeLogInterval(Number((e.currentTarget as HTMLSelectElement).value))}>
          {#each LOG_INTERVALS as iv (iv)}<option value={iv}>{fmtTime(iv)}</option>{/each}
        </select>
      </label>
      <small class="muted">
        Per interval: min, max, mean, σ of every signal. In memory: bounded (older intervals merge 10×, then 100×), ~{fmtBytesSize(sess.bytes)} now.
      </small>
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
      <details class="out" open={!logging?.active && !exporting}>
        <summary><h2>Output</h2></summary>
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
      </details>
      {#if isNative}
        {#if !logging?.active && !exporting}
          <button class="primary" onclick={startLog} disabled={!dev || !!dev.link.playback || (out.kind !== 'native' && !out.cfg.fields.length)}>
            ● Start logging to file
          </button>
          <small class="muted">To ~/Documents/Nebulosa. Stops cleanly when the disk runs low.</small>
        {/if}
      {:else}
        <small class="muted">Writing files needs the desktop app; here the session stays in memory (bounded). The preview shows what would be written.</small>
      {/if}
      <button class="ghost" onclick={newSession} disabled={!sess.has_data}>New session</button>
    </section>
    {:else}
    <TriggerPanel {trig} {available} onlevel50={level50} onarm={() => trig.rearm(store, view)} />

    {/if}

    <SignalsPanel {available} {signals} bind:shown bind:trace bind:axes bind:bufferMB bufferChoices={BUFFER_CHOICES} {bufferS} {rateHz} onbuffer={applyBuffer} />

    <MathPanel bind:draft={mathDraft} error={mathError} onapply={applyMath} onoff={mathOff} />

    <CalibrationPanel bind:cal ranges={CURRENT_RANGES} note={calNote} onapply={() => applyCal()} onreset={resetCal} />

    {#if error}
      <p class="error">{error}</p>
    {/if}
    {#if note}
      <p class="note small">{note} <button class="ghost small" onclick={() => (note = '')}>✕</button></p>
    {/if}
  </aside>

  <main bind:this={mainEl}>
    {#if appMode === 'logger'}
      <div class="logger-wrap">
        <LoggerView {store} session={sess} {signals} {axes} {trace} {wallOffset} bind:lv onopen={openInScope} />
      </div>
    {:else}
    <div class="strip">
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
          <button class="ghost small" onclick={() => view.clearMarkers()} title="Remove all markers">Clear</button>
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
        <button class="ghost small" onclick={openAnalysis} disabled={!hasData} title="Histogram, CDF, spectrum, worst window over the active dual marker (or the screen)">Analyze</button>
      </div>
      <span class="legend muted" title="Each pixel column covers many samples">
        {#if trace.mean}<span class="key"><i class="line"></i>mean</span>{/if}
        {#if trace.range}<span class="key"><i class="band"></i>min–max</span>{/if}
        {#if trace.sigma}<span class="key"><i class="sd"></i>±σ</span>{/if}
        <span class="key"><i class="gap"></i>gap</span>
      </span>
      <button class="hint muted linkish" onclick={() => (helpOpen = true)} title="All the keys and mouse gestures (?)"
        >wheel: time/div · shift+wheel: pan · space: live/hold · right-click: menu · ? keys</button
      >
    </div>
    {#if frontEndOff}
      <div class="banner warn">
        <b>Analog front end off</b>
        <span>{board?.apwr_ok ? 'Its supplies are off: the ADC reads nothing real.' : 'It needs 12 V: the barrel jack, or a USB-C charger set to 12 V (Board panel).'}</span>
        {#if board?.apwr_ok}<button class="small primary" onclick={frontEndOn}>Turn on</button>{/if}
      </div>
    {/if}
    <div class="overview-wrap">
      <Overview {store} {view} />
    </div>
    <div class="scope-wrap">
      <Scope bind:this={scope} {store} {view} {signals} {axes} {trace} {trig} onmenu={scopeMenu} />
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
    <div class="markers-wrap" style="height: min({listH}px, 22vh)">
      <div class="tabs">
        <button class:on={bottomTab === 'markers'} onclick={() => (bottomTab = 'markers')}>Markers <small>{view.markers.length}</small></button>
        <button class:on={bottomTab === 'events'} onclick={() => (bottomTab = 'events')}>Events <small>{trig.count.toLocaleString()}</small></button>
      </div>
      {#if bottomTab === 'events'}
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
        onclear={() => view.clearMarkers()}
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

  <div class="meter-col" class:hidden={!rightOpen}>
    <Meter
      {summary}
      accum={accumView}
      {signals}
      bind:scope={meterScope}
      bind:hold
      hasSelection={!!view.selection}
      onclear={clearAccum}
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
  .side-btn.on {
    color: var(--accent);
  }
  .brand {
    display: flex;
    gap: 8px;
    align-items: center;
  }
  .logo {
    color: var(--accent);
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
    border-radius: 50%;
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
    border-radius: 7px;
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
  .logger-wrap {
    flex: 1;
    min-height: 0;
    padding: 8px 12px 12px;
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
    border-radius: 4px;
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
  .autoset {
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
    border-radius: 6px;
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
    border-radius: 2px;
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
    border-radius: 6px 6px 0 0;
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
      border-radius: 6px;
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
      border-radius: 6px;
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
