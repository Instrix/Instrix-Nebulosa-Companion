<script lang="ts">
  import { onMount } from 'svelte';
  import Scope, { SIGNAL_INFO, type AxisCfg, type TraceCfg } from './lib/Scope.svelte';
  import Overview from './lib/Overview.svelte';
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
  } from './lib/nbp';
  import { fmtSI, parseSI } from './lib/si';
  import * as native from './lib/native';
  import * as web from './lib/webusb';
  import { DemoLink, demoRequested, demoSelection } from './lib/demo';

  /** full-rate buffer sizes, MB (4 bytes per sample) */
  const BUFFER_CHOICES = [64, 128, 256, 512, 1024, 2048];
  const BUFFER_KEY = 'nebulosa.bufferMB';
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
  const store = new SampleStore();
  const view = new View();
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
  let shown = $state<Record<Signal, boolean>>({ i: true, v: true, p: true });
  let trace = $state<TraceCfg>({ mean: true, range: true, sigma: false });
  const signals = $derived(SIGNALS.filter((s) => shown[s]));
  let axes = $state<Record<Signal, AxisCfg>>({
    i: { auto: true, min: 0, max: 0.1, log: false },
    v: { auto: true, min: 0, max: 5, log: false },
    p: { auto: true, min: 0, max: 0.5, log: false },
  });

  // ---- meter --------------------------------------------------------------
  let runMoments: Meta['run'] | null = null;
  let meterScope = $state<MeterScope>('window');
  let hold = $state(false);
  let summary = $state<Summary | null>(null);
  let markerRows = $state<MarkerRow[]>([]);
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

  async function applyBuffer() {
    try {
      localStorage.setItem(BUFFER_KEY, String(bufferMB));
    } catch {
      /* storage unavailable */
    }
    await dev?.link.setBuffer(bufferSamples);
  }

  // ---- statistics ---------------------------------------------------------
  function summarize(s: SpanStats, duration = s.t1 - s.t0): Summary | null {
    if (!s.i.n) return null;
    const ch = (m: SpanStats['i']) => ({ mean: m.mean, min: m.min, max: m.max, pp: m.pp, std: m.std, rms: m.rms });
    return { ch: { i: ch(s.i), v: ch(s.v), p: ch(s.p) }, n: s.i.n, duration };
  }

  let meterBusy = false;

  async function refreshMeter() {
    if (hold || meterBusy) return;
    meterBusy = true;
    try {
      if (meterScope === 'run') {
        const r = runMoments;
        summary = r && r.i.n ? summarize({ i: Moments.from(r.i), v: Moments.from(r.v), p: Moments.from(r.p), t0: 0, t1: 0, charge: 0, energy: 0 }, accum?.seconds ?? NaN) : null;
      } else if (meterScope === 'selection' && view.selection) summary = summarize(await store.stats(...view.selection));
      else if (store.empty) summary = null;
      else summary = summarize(await store.stats(...view.span(store)));
      markerRows = await Promise.all(
        view.markers.map(async (m): Promise<MarkerRow> => {
          const base = { id: m.id, name: markerName(m), color: markerColor(m) };
          if (m.t1 == null) {
            const v = await store.valueAt(m.t0);
            return { ...base, dt: null, ch: Object.fromEntries(SIGNALS.map((s) => [s, v ? { mean: v[s], integral: NaN } : null])) as MarkerRow['ch'] };
          }
          const st = await store.stats(m.t0, m.t1);
          const integral: Record<Signal, number> = { i: st.charge, v: NaN, p: st.energy };
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

  async function connect(c: Candidate) {
    busy = true;
    error = '';
    try {
      await disconnect();
      const link: Link = await c.connect({
        onMeta,
        onClosed: (reason) => {
          error = `Disconnected: ${reason}`;
          reset();
        },
      });
      const d = new Device(link);
      dev = d;
      await link.setBuffer(bufferSamples);
      await link.setCalibration($state.snapshot(cal));
      store.attach(link);
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
    store.attach(null);
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

  function onMeta(m: Meta) {
    store.update(m);
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
      const demo = new DemoLink({ onMeta, onClosed: () => undefined });
      demo.setBuffer(bufferSamples);
      demo.setCalibration($state.snapshot(cal));
      store.attach(demo);
      stopDemo = () => demo.close();
      // screenshot helpers: &log=i,p  &trace=mean,range,sigma
      const q = new URLSearchParams(location.search);
      for (const sg of (q.get('log') ?? '').split(',')) if (sg in axes) axes[sg as Signal].log = true;
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
        <span title="Each pixel column covers many samples: the line is their mean, the bands their min–max range and ±1 standard deviation">Trace</span>
        <div class="seg">
          <button class:on={trace.mean} onclick={() => (trace.mean = !trace.mean)} title="Mean line">Mean</button>
          <button class:on={trace.range} onclick={() => (trace.range = !trace.range)} title="Min–max band">Min–max</button>
          <button class:on={trace.sigma} onclick={() => (trace.sigma = !trace.sigma)} title="Mean ± 1 standard deviation">±σ</button>
        </div>
      </div>
      {#each signals as s (s)}
        {@const a = axes[s]}
        {@const unit = SIGNAL_INFO[s].unit}
        <div class="axis">
          <span style="color: var({SIGNAL_INFO[s].color})">{SIGNAL_INFO[s].name} axis</span>
          <div class="seg">
            <button class:on={a.log} onclick={() => (a.log = !a.log)} title="Logarithmic scale">Log</button>
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
        Sample buffer (RAM)
        <select bind:value={bufferMB} onchange={applyBuffer}>
          {#each BUFFER_CHOICES as mb (mb)}<option value={mb}>{mb >= 1024 ? `${mb / 1024} GB` : `${mb} MB`}</option>{/each}
        </select>
        <small class="muted">every sample: {fmtTime(bufferS)} at {fmtRate(rateHz || 2e6)}</small>
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
          disabled={view.divIndex === TIME_DIVS.length - 1 || TIME_DIVS[view.divIndex + 1] * 10 > Math.max(bufferS, 1)}
          title="Slower (-, wheel)">+</button
        >
      </div>
      {#if rateHz}<span class="mono muted">{fmtRate(rateHz)} · N={1 << log2n}</span>{/if}
      <div class="tdiv">
        <button class="ghost small" onclick={addSingleMarker} disabled={store.empty} title="Single marker (M, or double-click a plot)">+ Marker</button>
        <button class="ghost small" onclick={addDualMarker} disabled={store.empty} title="Dual marker (D, or drag across a plot)">+ Dual</button>
        {#if view.markers.length}
          <button class="ghost small" onclick={() => view.clearMarkers()} title="Remove all markers">Clear</button>
        {/if}
      </div>
      <span class="legend muted" title="Each pixel column covers many samples">
        {#if trace.mean}<span class="key"><i class="line"></i>mean</span>{/if}
        {#if trace.range}<span class="key"><i class="band"></i>min–max</span>{/if}
        {#if trace.sigma}<span class="key"><i class="sd"></i>±σ</span>{/if}
        <span class="key"><i class="gap"></i>gap</span>
      </span>
      <span class="hint muted" title="Drag across a plot: dual marker. Double-click: single marker. M / D: add a marker. Del: delete the active one. Esc: deselect."
        >wheel: time/div · shift+wheel: pan · space: live/hold · ⓘ</span
      >
    </div>
    <div class="overview-wrap">
      <Overview {store} {view} />
    </div>
    <div class="scope-wrap">
      <Scope {store} {view} {signals} {axes} {trace} />
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
      markers={markerRows}
      active={view.active}
      onpick={(id) => (view.active = id)}
      ondelete={(id) => view.remove(id)}
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
