<script lang="ts">
  import { onMount } from 'svelte';
  import Scope, { SIGNAL_INFO, type AxisCfg, type TraceMode } from './lib/Scope.svelte';
  import Overview from './lib/Overview.svelte';
  import Meter, { type AccumView, type Scope as MeterScope, type Summary } from './lib/Meter.svelte';
  import { View, TIME_DIVS, fmtTime } from './lib/view.svelte';
  import { PointStore, addPoints, emptyStats, type SpanStats } from './lib/store';
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
    type Points,
    type Signal,
  } from './lib/nbp';
  import { fmtSI, parseSI } from './lib/si';
  import * as native from './lib/native';
  import * as web from './lib/webusb';
  import { demoRequested, demoSelection, startDemo } from './lib/demo';

  const TARGET_POINTS_PER_S = 4000;
  const HISTORY_CHOICES = [60, 120, 300, 600];
  const CURRENT_RANGES = 6;

  // ---- connection ---------------------------------------------------------
  let candidates = $state<Candidate[]>([]);
  let scanning = $state(false);
  let manualHost = $state('');
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
  const store = new PointStore();
  const view = new View();
  let historyS = $state(120);
  let shown = $state<Record<Signal, boolean>>({ i: true, v: true, p: true });
  let traceMode = $state<TraceMode>('both');
  const signals = $derived(SIGNALS.filter((s) => shown[s]));
  let axes = $state<Record<Signal, AxisCfg>>({
    i: { auto: true, min: 0, max: 0.1 },
    v: { auto: true, min: 0, max: 5 },
    p: { auto: true, min: 0, max: 0.5 },
  });

  // ---- meter --------------------------------------------------------------
  let runStats: SpanStats = emptyStats();
  let meterScope = $state<MeterScope>('window');
  let hold = $state(false);
  let summary = $state<Summary | null>(null);
  let accum: Accum | null = null;
  let accumBase: Accum = { charge: 0, energy: 0, seconds: 0 };
  let accumView = $state<AccumView | null>(null);
  let streamId = -1;

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

  // ---- statistics ---------------------------------------------------------
  function summarize(s: SpanStats): Summary | null {
    if (!s.i.n) return null;
    const ch = (m: SpanStats['i']) => ({ mean: m.mean, min: m.min, max: m.max, pp: m.pp, std: m.std, rms: m.rms });
    return { ch: { i: ch(s.i), v: ch(s.v), p: ch(s.p) }, n: s.i.n, duration: s.t1 - s.t0 };
  }

  function refreshMeter() {
    if (hold) return;
    if (meterScope === 'run') summary = summarize(runStats);
    else if (meterScope === 'selection' && view.selection) summary = summarize(store.stats(...view.selection));
    else if (store.empty) summary = null;
    else summary = summarize(store.stats(...view.span(store)));
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
    runStats = emptyStats();
    view.selection = null;
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
    } else if (e.key === 'Escape') view.selection = null;
    else if (e.key === '+' || e.key === '=') view.zoom(store, -1);
    else if (e.key === '-') view.zoom(store, 1);
    else if (e.key === 'ArrowLeft') view.pan(store, -0.1);
    else if (e.key === 'ArrowRight') view.pan(store, 0.1);
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

  async function connect(c: Candidate) {
    busy = true;
    error = '';
    try {
      await disconnect();
      const link: Link = await c.connect({
        onPoints,
        onMeta,
        onClosed: (reason) => {
          error = `Disconnected: ${reason}`;
          reset();
        },
      });
      const d = new Device(link);
      dev = d;
      await link.setCalibration($state.snapshot(cal));
      info = await d.info();
      clearData();
      await applyDecimation();
    } catch (e) {
      error = String(e);
      await disconnect();
    } finally {
      busy = false;
    }
  }

  function reset() {
    dev = null;
    info = null;
    streaming = false;
    stats = null;
    throughput = 0;
  }

  async function disconnect() {
    const d = dev;
    reset();
    if (d) await d.link.close().catch(() => undefined);
  }

  function onPoints(p: Points) {
    addPoints(runStats, p);
    store.append(p, historyS);
  }

  function onMeta(m: Meta) {
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
    await dev.link.setBucket(Math.max(1, Math.round(rateHz / TARGET_POINTS_PER_S)));
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

  function fmtRate(hz: number) {
    return hz >= 1e6 ? `${hz / 1e6} MS/s` : hz >= 1e3 ? `${(hz / 1e3).toFixed(hz % 1000 ? 2 : 0)} kS/s` : `${hz} S/s`;
  }

  function fmtBytes(bps: number) {
    return bps >= 1e6 ? `${(bps / 1e6).toFixed(2)} MB/s` : `${(bps / 1e3).toFixed(0)} kB/s`;
  }

  /** an SI text field: shows `value`, writes back what parses */
  function siInput(node: HTMLInputElement, p: { get: () => number; set: (v: number) => void; unit: string }) {
    let cur = p;
    const show = () => (node.value = fmtSI(cur.get(), cur.unit, 4));
    const commit = () => {
      const v = parseSI(node.value);
      if (isFinite(v)) cur.set(v);
      show();
    };
    show();
    node.addEventListener('change', commit);
    return {
      update(n: typeof p) {
        cur = n;
        if (document.activeElement !== node) show();
      },
      destroy() {
        node.removeEventListener('change', commit);
      },
    };
  }

  onMount(() => {
    scan();
    const t = setInterval(refreshMeter, 250);
    let stopDemo = () => {};
    if (demoRequested) {
      streaming = true;
      rateHz = 2e6;
      log2n = 0;
      stopDemo = startDemo(onPoints, onMeta);
      const sel = demoSelection();
      if (sel)
        setTimeout(() => {
          view.hold(store);
          view.selection = [store.tEnd - sel[0], store.tEnd - sel[1]];
        }, 3000);
    }
    return () => {
      clearInterval(t);
      stopDemo();
    };
  });
</script>

<svelte:window onkeydown={onKey} />

<div class="app">
  <header>
    <div class="brand">
      <span class="logo">◉</span>
      <strong>Nebulosa</strong>
      <span class="muted">Companion</span>
    </div>
    <div class="status">
      {#if dev}
        <span class="dot ok"></span>{dev.link.description}
        <button class="ghost" onclick={disconnect}>Disconnect</button>
      {:else}
        <span class="dot"></span><span class="muted">Not connected</span>
      {/if}
    </div>
  </header>

  <aside>
    {#if !dev}
      <section>
        <h2>
          Devices
          <button class="ghost small" onclick={scan} disabled={scanning}>{scanning ? 'Scanning…' : 'Refresh'}</button>
        </h2>
        {#each candidates as c (c.key)}
          <button class="device" onclick={() => connect(c)} disabled={busy}>
            <span>{c.label}</span>
            <small>{c.detail}</small>
          </button>
        {:else}
          <p class="muted small">{scanning ? 'Looking…' : 'No devices found.'}</p>
        {/each}

        {#if isNative}
          <form
            class="row"
            onsubmit={(e) => {
              e.preventDefault();
              if (manualHost.trim()) connect(native.tcpCandidate(manualHost.trim()));
            }}
          >
            <input placeholder="IP address or host" bind:value={manualHost} />
            <button type="submit" disabled={busy || !manualHost.trim()}>Connect</button>
          </form>
        {:else if web.webUsbAvailable}
          <button class="primary" onclick={pair} disabled={busy}>Pair USB device…</button>
          <p class="muted small">Browser build: USB only. Ethernet needs the desktop app.</p>
        {:else}
          <p class="warn small">This browser has no WebUSB. Use Chrome or Edge, or the desktop app.</p>
        {/if}
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
      </section>

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

    <section>
      <h2>Signals</h2>
      <div class="chips">
        {#each SIGNALS as s (s)}
          <label class="chip" style="--c: var({SIGNAL_INFO[s].color})">
            <input type="checkbox" bind:checked={shown[s]} />
            {SIGNAL_INFO[s].name}
          </label>
        {/each}
      </div>
      <div class="axis">
        <span title="Each pixel column covers many samples: the line is their mean, the band their min–max range">Trace</span>
        <div class="seg">
          <button class:on={traceMode === 'both'} onclick={() => (traceMode = 'both')} title="Mean line and min–max band">Both</button>
          <button class:on={traceMode === 'mean'} onclick={() => (traceMode = 'mean')} title="Mean line only">Mean</button>
          <button class:on={traceMode === 'range'} onclick={() => (traceMode = 'range')} title="Min–max band only">Min–max</button>
        </div>
      </div>
      {#each signals as s (s)}
        {@const a = axes[s]}
        {@const unit = SIGNAL_INFO[s].unit}
        <div class="axis">
          <span style="color: var({SIGNAL_INFO[s].color})">{SIGNAL_INFO[s].name} axis</span>
          <div class="seg">
            <button class:on={a.auto} onclick={() => (a.auto = true)}>Auto</button>
            <button class:on={!a.auto} onclick={() => (a.auto = false)}>Fixed</button>
          </div>
        </div>
        {#if !a.auto}
          <div class="row">
            <label>Min <input use:siInput={{ get: () => a.min, set: (v) => (a.min = v), unit }} /></label>
            <label>Max <input use:siInput={{ get: () => a.max, set: (v) => (a.max = v), unit }} /></label>
          </div>
        {/if}
      {/each}
      <label>
        History kept
        <select bind:value={historyS}>
          {#each HISTORY_CHOICES as h (h)}<option value={h}>{fmtTime(h)} min</option>{/each}
        </select>
      </label>
    </section>

    <section>
      <details>
        <summary><h2>Calibration</h2></summary>
        <div class="cal">
          <p class="muted small">Firmware scaling by default. I = code × full scale / 16384; V = code × scale + offset.</p>
          {#each Array.from({ length: CURRENT_RANGES }, (_, r) => r) as r (r)}
            <label class="inline">
              Range {r} full scale
              <input use:siInput={{ get: () => cal.i_fs[r], set: (v) => (cal.i_fs[r] = v), unit: 'A' }} />
            </label>
          {/each}
          <label class="inline">
            Voltage scale (per code)
            <input use:siInput={{ get: () => cal.v_scale, set: (v) => (cal.v_scale = v), unit: 'V' }} />
          </label>
          <label class="inline">
            Voltage offset
            <input use:siInput={{ get: () => cal.v_offset, set: (v) => (cal.v_offset = v), unit: 'V' }} />
          </label>
          <div class="row">
            <button class="primary" onclick={() => applyCal()}>Apply</button>
            <button class="ghost" onclick={resetCal}>Firmware defaults</button>
          </div>
          {#if calNote}<p class="muted small">{calNote}</p>{/if}
        </div>
      </details>
    </section>

    {#if error}
      <p class="error">{error}</p>
    {/if}
  </aside>

  <main>
    <div class="strip">
      <span class="badge" class:run={streaming} class:stop={!streaming}>{streaming ? 'RUN' : 'STOP'}</span>
      <button
        class="badge btn"
        class:live={view.live}
        class:hold={!view.live}
        onclick={toggleLive}
        title="Follow the newest data (Space)">{view.live ? 'LIVE' : 'HOLD'}</button
      >
      <div class="tdiv">
        <button class="ghost small" onclick={() => view.zoom(store, -1)} disabled={view.divIndex === 0} title="Faster (+, wheel)">−</button>
        <span class="mono">{fmtTime(view.timeDiv)}/div</span>
        <button
          class="ghost small"
          onclick={() => view.zoom(store, 1)}
          disabled={view.divIndex === TIME_DIVS.length - 1 || TIME_DIVS[view.divIndex + 1] * 10 > historyS}
          title="Slower (-, wheel)">+</button
        >
      </div>
      {#if rateHz}<span class="mono muted">{fmtRate(rateHz)} · N={1 << log2n}</span>{/if}
      {#if view.selection}
        <span class="mono sel">
          sel Δt {fmtTime(view.selection[1] - view.selection[0])}
          <button class="ghost small" onclick={() => (view.selection = null)} title="Clear (Esc, double-click)">✕</button>
        </span>
      {/if}
      <span class="hint muted">drag: select · wheel: time/div · shift+wheel: pan · space: live/hold</span>
    </div>
    <div class="overview-wrap">
      <Overview {store} {view} />
    </div>
    <div class="scope-wrap">
      <Scope {store} {view} {signals} {axes} mode={traceMode} />
    </div>
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

  <div class="meter-col">
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
  .app {
    display: grid;
    grid-template-columns: 280px 1fr 280px;
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
  .brand {
    display: flex;
    gap: 8px;
    align-items: baseline;
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
  aside {
    border-right: 1px solid var(--border);
    background: var(--panel);
    overflow-y: auto;
    padding: 8px 16px 16px;
  }
  .meter-col {
    border-left: 1px solid var(--border);
    background: var(--bg);
    overflow-y: auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
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
  main {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
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
  .tdiv {
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }
  .mono {
    font-family: ui-monospace, monospace;
    font-variant-numeric: tabular-nums;
  }
  .sel {
    color: var(--sel);
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }
  .hint {
    margin-left: auto;
    font-size: 11px;
  }
  .overview-wrap {
    padding: 8px 12px 0;
  }
  .scope-wrap {
    flex: 1;
    min-height: 0;
    padding: 8px 12px 12px;
  }
  footer {
    display: flex;
    flex-wrap: wrap;
    gap: 16px;
    padding: 8px 16px;
    border-top: 1px solid var(--border);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    color: var(--muted);
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
  .bad {
    color: var(--danger);
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
</style>
