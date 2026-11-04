<script lang="ts">
  // Side-channel power analysis, ChipWhisperer-style: capture a campaign of
  // power traces aligned on a trigger, then look for data-dependent leakage —
  // mean and spread, a TVLA t-test, and timing. Export to .npz for other
  // tools. Everything here is for authorised testing, teaching and research,
  // and runs out of the box on the demo target (a simulated leaky AES).
  import uPlot from 'uplot';
  import 'uplot/dist/uPlot.min.css';
  import { onDestroy } from 'svelte';
  import type { Link } from './device';
  import { isNative } from './device';
  import type { AlignResult, ScaInfo, ScaResult, Signal } from './nbp';
  import { SIGNAL_INFO } from './Scope.svelte';
  import { fmtSI } from './si';
  import { fmtTime } from './view.svelte';
  import type { TriggerState } from './trigger.svelte';
  import { runCapture, keyOrDefault, type Plan } from './capture';
  import { demoTarget, nativeSerialPorts, nativeSerialTarget, webSerialTarget, type Target } from './targets';
  import { toHex } from './aes';
  import type { DemoTarget } from './demo';

  let {
    link,
    demo,
    trig,
    dt,
    signals,
    streaming,
    onnote,
  }: {
    link: Link | null;
    /** the demo device's simulated target, when the demo is connected */
    demo: DemoTarget | null;
    /** the app's trigger: a capture sets it to an edge / pulse on the source */
    trig: TriggerState;
    /** s per sample (1 / rate) */
    dt: number;
    signals: Signal[];
    streaming: boolean;
    onnote: (m: string) => void;
  } = $props();

  type View = 'traces' | 'ttest' | 'timing' | 'cpa' | 'export';
  let view = $state<View>('traces');

  // CPA: a key-byte attack and a full-key recovery
  let cpaByte = $state(0);
  let cpa = $state.raw<Extract<ScaResult, { kind: 'cpa' }> | null>(null);
  let cpaBusy = $state(false);
  /** recovered key bytes (null until found); best corr per byte */
  let recovered = $state<(number | null)[]>(Array(16).fill(null));
  let recoveredCorr = $state<number[]>(Array(16).fill(0));
  let recoverAll = $state(false);

  // ---- target -------------------------------------------------------------
  let target = $state.raw<Target | null>(null);
  let targetKind = $state<'demo' | 'serial'>('demo');
  let keyHex = $state('2b7e151628aed2a6abf7158809cf4f3c');
  /** the demo target's key is known, so matches can be checked */
  const knownKey = $derived(targetKind === 'demo' && demo ? keyOrDefault(keyHex) : null);
  const knownByte = (b: number) => (knownKey && b < knownKey.length ? knownKey[b] : 256);
  const hex2 = (v: number) => v.toString(16).padStart(2, '0');
  let password = $state('nebulosa');
  let ports = $state<{ path: string; label: string }[]>([]);
  let portPath = $state('');
  let baud = $state(38400);
  let connecting = $state(false);

  $effect(() => {
    // the demo target, ready as soon as the demo is connected
    if (targetKind === 'demo' && demo) {
      demo.key = keyOrDefault(keyHex) as Uint8Array<ArrayBuffer>;
      demo.password = new TextEncoder().encode(password);
      target = demoTarget(demo);
    }
  });

  async function listPorts() {
    try {
      ports = await nativeSerialPorts();
      if (!portPath && ports[0]) portPath = ports[0].path;
    } catch (e) {
      onnote(String(e));
    }
  }

  async function connectSerial() {
    connecting = true;
    try {
      target = isNative ? await nativeSerialTarget(portPath, baud) : await webSerialTarget(baud);
    } catch (e) {
      onnote(String(e));
    } finally {
      connecting = false;
    }
  }

  // ---- capture ------------------------------------------------------------
  type PlanKind = 'aes-random' | 'aes-tvla' | 'password';
  let planKind = $state<PlanKind>('aes-tvla');
  let traces = $state(200);
  let sampleLen = $state(2000);
  let level = $state(0.004);
  let capturing = $state(false);
  let stopFlag = false;
  let progress = $state(0);
  let info = $state.raw<ScaInfo | null>(null);
  let captureError = $state('');
  let lastLatency = $state(0);

  const source: Signal = 'i';
  const sourceUnit = $derived(SIGNAL_INFO[source].unit);
  const captureMs = $derived(sampleLen * dt * 1e3);

  /** the password guesses: each printable character tried at the first wrong position */
  function passwordGuesses(): string[] {
    const known = ''; // the attacker knows nothing at the start; one position at a time is a deeper lesson
    const chars = 'abcdefghijklmnopqrstuvwxyz0123456789';
    return [...chars].map((c) => known + c);
  }

  function plan(): Plan {
    if (planKind === 'aes-random') return { kind: 'aes-random' };
    if (planKind === 'password') return { kind: 'password', guesses: passwordGuesses() };
    return { kind: 'aes-tvla', fixed: keyOrDefault('00112233445566778899aabbccddeeff') };
  }

  async function capture() {
    if (!link || !target) {
      onnote('connect a target first');
      return;
    }
    if (!streaming) {
      onnote('start the stream first (the capture needs live samples)');
      return;
    }
    // a rising edge on the operation marks a trace's start (the op's own start);
    // the window then holds the whole operation. Detection on without changing
    // the display mode. The timing attack reads each op's duration from the
    // captured trace, not from the trigger.
    const kind = 'edge' as const;
    trig.source = source;
    trig.kind = kind;
    trig.slope = 'rise';
    trig.polarity = 'high';
    trig.cond = 'any';
    trig.level = level;
    trig.log = true;

    capturing = true;
    stopFlag = false;
    captureError = '';
    progress = 0;
    try {
      await runCapture({
        link,
        target,
        samples: sampleLen,
        source,
        plan: plan(),
        traces,
        triggerKind: kind,
        dt,
        onProgress: (n, ti) => {
          progress = n;
          lastLatency = ti.latency;
        },
        running: () => !stopFlag,
      });
      info = await link.sca.info();
      await refresh();
    } catch (e) {
      captureError = String(e instanceof Error ? e.message : e);
    } finally {
      capturing = false;
    }
  }

  // ---- align --------------------------------------------------------------
  let aligning = $state(false);
  let alignInfo = $state<AlignResult | null>(null);
  let alignFrom = $state(0);
  let alignTo = $state(400);
  let maxShift = $state(50);

  async function align() {
    if (!link || !info?.n) return;
    aligning = true;
    try {
      alignInfo = await link.sca.align(alignFrom, alignTo, maxShift);
      await refresh();
    } catch (e) {
      onnote(String(e));
    } finally {
      aligning = false;
    }
  }

  async function clearAlign() {
    if (!link) return;
    await link.sca.clearAlign();
    alignInfo = null;
    await refresh();
  }

  // ---- views / charts -----------------------------------------------------
  let result = $state.raw<ScaResult | null>(null);
  let chartEl = $state<HTMLDivElement>();
  let chart: uPlot | undefined;

  async function refresh() {
    if (!link || !info?.n) {
      result = null;
      drawChart();
      return;
    }
    try {
      if (view === 'timing' || view === 'cpa') result = null;
      else result = await link.sca.query(view === 'ttest' ? { kind: 'ttest' } : { kind: 'overview', show: 20 });
    } catch (e) {
      onnote(String(e));
    }
    drawChart();
  }

  $effect(() => {
    void view;
    refresh();
  });

  function css(n: string) {
    return getComputedStyle(document.documentElement).getPropertyValue(n).trim() || '#888';
  }

  function drawChart() {
    chart?.destroy();
    chart = undefined;
    if (!chartEl) return;
    const r = result;
    const axisX = { stroke: css('--muted'), grid: { stroke: css('--grid') }, ticks: { show: false }, values: (_u: uPlot, v: number[]) => v.map((x) => fmtTime(x)) };
    const axisY = { stroke: css('--muted'), grid: { stroke: css('--grid') }, ticks: { show: false }, size: 70 };
    const W = chartEl.clientWidth;
    const H = chartEl.clientHeight;
    if (view === 'timing') return drawTiming(W, H);
    if (view === 'cpa') return drawCpa(W, H);
    if (!r || r.kind === 'empty') return;

    if (r.kind === 'overview') {
      const x = r.mean.map((_, i) => i * r.dt);
      const col = css(SIGNAL_INFO[source].color);
      const data: uPlot.AlignedData = [x, r.mean, r.mean.map((m, i) => m + r.std[i]), r.mean.map((m, i) => m - r.std[i]), ...r.traces];
      chart = new uPlot(
        {
          width: W,
          height: H,
          legend: { show: false },
          cursor: { points: { show: false } },
          scales: { x: { time: false } },
          series: [
            {},
            { stroke: col, width: 2, points: { show: false } },
            { stroke: 'transparent', fill: col + '22', points: { show: false } },
            { stroke: 'transparent', fill: col + '22', points: { show: false } },
            ...r.traces.map(() => ({ stroke: css('--muted') + '55', width: 0.5, points: { show: false } })),
          ],
          bands: [{ series: [2, 3] }],
          axes: [axisX, { ...axisY, values: (_u, v) => v.map((y) => fmtSI(y, sourceUnit, 3)) }],
        },
        data,
        chartEl,
      );
    } else if (r.kind === 'ttest') {
      const x = r.t.map((_, i) => i * r.dt);
      const thr = x.map(() => 4.5);
      chart = new uPlot(
        {
          width: W,
          height: H,
          legend: { show: false },
          cursor: { points: { show: false } },
          scales: { x: { time: false } },
          series: [
            {},
            { stroke: css('--accent'), width: 1, points: { show: false } },
            { stroke: css('--danger') + '99', width: 1, dash: [4, 4], points: { show: false } },
            { stroke: css('--danger') + '99', width: 1, dash: [4, 4], points: { show: false } },
          ],
          axes: [axisX, { ...axisY, values: (_u, v) => v.map((y) => y.toFixed(1)) }],
        },
        [x, r.t, thr, thr.map((v) => -v)],
        chartEl,
      );
    }
  }

  // timing: the op duration per guess (samples above a threshold), averaged
  let timing = $state<{ guesses: string[]; mean: number[]; n: number[]; best: number } | null>(null);
  async function loadTiming() {
    if (!link || !info?.n) {
      timing = null;
      return;
    }
    const [metas, res] = await Promise.all([link.sca.meta(), link.sca.query({ kind: 'timing', threshold: level })]);
    const durations = res.kind === 'timing' ? res.durations : [];
    const map = new Map<string, number[]>();
    metas.forEach((m, i) => {
      const g = new TextDecoder().decode(Uint8Array.from(m.input).subarray(0, 16)).replace(/\0+$/, '') || '∅';
      (map.get(g) ?? map.set(g, []).get(g)!).push(durations[i] ?? 0);
    });
    const guesses = [...map.keys()].sort();
    const mean = guesses.map((g) => map.get(g)!.reduce((a, b) => a + b, 0) / map.get(g)!.length);
    const n = guesses.map((g) => map.get(g)!.length);
    const best = mean.indexOf(Math.max(...mean));
    timing = { guesses, mean, n, best };
  }

  function drawTiming(W: number, H: number) {
    if (!timing || !chartEl) return;
    const x = timing.guesses.map((_, i) => i);
    chart = new uPlot(
      {
        width: W,
        height: H,
        legend: { show: false },
        scales: { x: { time: false } },
        series: [
          {},
          {
            stroke: css('--accent'),
            fill: css('--accent') + '55',
            width: 1,
            points: { show: true, size: 6 },
            paths: uPlot.paths.bars!({ size: [0.7] }),
          },
        ],
        axes: [
          { stroke: css('--muted'), grid: { show: false }, ticks: { show: false }, values: (_u, v) => v.map((i) => timing!.guesses[i] ?? '') },
          { stroke: css('--muted'), grid: { stroke: css('--grid') }, size: 70, values: (_u, v) => v.map((y) => fmtTime(y)) },
        ],
      },
      [x, timing.mean],
      chartEl,
    );
  }

  /** the 256 correlations, best in red; a line per guess */
  function drawCpa(W: number, H: number) {
    if (!cpa || !chartEl) return;
    const x = cpa.corr.map((_, i) => i);
    const best = cpa.best;
    const col = cpa.correct === false ? css('--danger') : css('--accent');
    // the best guess as a second series, so it draws on top in a bold colour
    const bestSeries = cpa.corr.map((c, i) => (i === best ? c : null));
    chart = new uPlot(
      {
        width: W,
        height: H,
        legend: { show: false },
        cursor: { points: { show: false } },
        scales: { x: { time: false }, y: { range: (_u, _lo, hi) => [0, Math.max(0.2, (hi ?? 0.2) * 1.1)] } },
        series: [
          {},
          { stroke: css('--muted') + '99', width: 1, points: { show: false }, paths: uPlot.paths.bars!({ size: [0.9] }) },
          { stroke: col, fill: col, width: 1, points: { show: true, size: 7 }, paths: uPlot.paths.bars!({ size: [1] }) },
        ],
        axes: [
          { stroke: css('--muted'), grid: { show: false }, ticks: { show: false }, values: (_u, v) => v.map((i) => (i % 32 === 0 ? '0x' + (i & 0xff).toString(16).padStart(2, '0') : '')) },
          { stroke: css('--muted'), grid: { stroke: css('--grid') }, size: 56, values: (_u, v) => v.map((y) => y.toFixed(2)) },
        ],
      },
      [x, cpa.corr, bestSeries],
      chartEl,
    );
  }

  $effect(() => {
    if (view === 'timing') loadTiming().then(() => drawChart());
  });

  // ---- CPA ----------------------------------------------------------------
  async function runCpaByte(b: number): Promise<Extract<ScaResult, { kind: 'cpa' }> | null> {
    if (!link || !info?.n) return null;
    const r = await link.sca.query({ kind: 'cpa', byte: b, known: knownByte(b) });
    if (r.kind !== 'cpa') {
      onnote(r.kind === 'empty' ? r.reason : 'CPA failed');
      return null;
    }
    return r;
  }

  async function runCpa() {
    cpaBusy = true;
    try {
      const r = await runCpaByte(cpaByte);
      if (r) {
        cpa = r;
        recovered[r.byte] = r.best;
        recoveredCorr[r.byte] = r.best_corr;
        recovered = [...recovered];
        recoveredCorr = [...recoveredCorr];
      }
      drawChart();
    } finally {
      cpaBusy = false;
    }
  }

  async function recoverKey() {
    cpaBusy = true;
    recoverAll = true;
    try {
      for (let b = 0; b < 16; b++) {
        cpaByte = b;
        const r = await runCpaByte(b);
        if (!r) break;
        cpa = r;
        recovered[b] = r.best;
        recoveredCorr[b] = r.best_corr;
        recovered = [...recovered];
        recoveredCorr = [...recoveredCorr];
        drawChart();
        await new Promise((res) => setTimeout(res, 0));
      }
    } finally {
      cpaBusy = false;
      recoverAll = false;
    }
  }

  const recoveredHex = $derived(recovered.map((b) => (b == null ? '··' : hex2(b))).join(' '));
  const keyComplete = $derived(recovered.every((b) => b != null));
  const keyCorrect = $derived(knownKey && keyComplete && recovered.every((b, i) => b === knownKey[i]));

  // ---- export -------------------------------------------------------------
  async function exportNpz() {
    if (!link || !info?.n) return;
    try {
      const bytes = await link.sca.exportNpz();
      const name = `nebulosa-traces-${new Date().toISOString().replace(/[:.]/g, '-').slice(0, 19)}.npz`;
      if (isNative) {
        const native = await import('./native');
        const path = await native.pickSave(name, 'NumPy archive', 'npz');
        if (!path) return;
        await native.saveBytes(path, bytes);
        onnote(`Saved ${info.n} traces`);
      } else {
        const a = document.createElement('a');
        a.href = URL.createObjectURL(new Blob([bytes as BlobPart], { type: 'application/octet-stream' }));
        a.download = name;
        a.click();
        setTimeout(() => URL.revokeObjectURL(a.href), 1000);
      }
    } catch (e) {
      onnote(String(e));
    }
  }

  const ro = typeof ResizeObserver !== 'undefined' ? new ResizeObserver(() => drawChart()) : null;
  $effect(() => {
    if (chartEl && ro) ro.observe(chartEl);
    return () => ro?.disconnect();
  });
  onDestroy(() => {
    chart?.destroy();
    if (targetKind === 'serial') target?.close();
  });
</script>

<div class="sca">
  <div class="disclaimer">
    <b>Beta.</b> Side-channel analysis needs many samples per operation, so this works on <b>slow targets</b>: clock the target
    down to ≲ 500 kHz (or a slow software routine). At 2 MS/s it cannot break a full-speed microcontroller — it is for SPA, timing,
    leakage detection (TVLA) and CPA on slow targets, plus teaching. For authorised testing and research only.
  </div>
  <aside class="panels">
    <section>
      <h3>Target</h3>
      <div class="seg">
        <button class:on={targetKind === 'demo'} onclick={() => (targetKind = 'demo')} disabled={!demo}>Demo</button>
        <button class:on={targetKind === 'serial'} onclick={() => (targetKind = 'serial')}>Serial</button>
      </div>
      {#if targetKind === 'demo'}
        {#if demo}
          <label>AES key <input class="mono" bind:value={keyHex} spellcheck="false" /></label>
          <label>Password <input class="mono" bind:value={password} spellcheck="false" /></label>
          <p class="muted small">A simulated microcontroller: a leaky AES and a byte-by-byte password check. Connect the demo device to use it.</p>
        {:else}
          <p class="muted small">Connect the demo device (▸ Showcases, or the connect panel) for the simulated target.</p>
        {/if}
      {:else}
        {#if isNative}
          <div class="row">
            <select bind:value={portPath}>
              {#each ports as p (p.path)}<option value={p.path}>{p.label}</option>{/each}
            </select>
            <button class="ghost small" onclick={listPorts}>Scan</button>
          </div>
        {/if}
        <label>Baud <input type="number" bind:value={baud} /></label>
        <button class="primary small" onclick={connectSerial} disabled={connecting}>{connecting ? 'Opening…' : 'Connect (SimpleSerial)'}</button>
        <p class="muted small">SimpleSerial v1 ({isNative ? 'a serial port' : 'Web Serial — Chrome / Edge'}): a target that answers <code>p</code> (encrypt) and <code>c</code> (password).</p>
      {/if}
      <p class="status" class:ok={target}>{target ? target.name : 'No target'}</p>
    </section>

    <section>
      <h3>Capture</h3>
      <label>What
        <select bind:value={planKind}>
          <option value="aes-tvla">AES — fixed vs random (TVLA)</option>
          <option value="aes-random">AES — random plaintext</option>
          <option value="password">Password — timing</option>
        </select>
      </label>
      <div class="row">
        <label>Traces <input type="number" min="2" bind:value={traces} /></label>
        <label>Samples <input type="number" min="16" bind:value={sampleLen} /></label>
      </div>
      <label>Trigger level ({sourceUnit})
        <input class="mono" value={level} oninput={(e) => (level = Number((e.target as HTMLInputElement).value) || level)} />
      </label>
      <p class="muted small">A trace is {sampleLen.toLocaleString()} samples ({fmtTime(captureMs / 1e3)}) from the rising edge of {SIGNAL_INFO[source].name} through {fmtSI(level, sourceUnit, 3)}.</p>
      {#if capturing}
        <button class="primary" onclick={() => (stopFlag = true)}>■ Stop ({progress} / {traces})</button>
        <div class="bar"><span style="width: {(100 * progress) / traces}%"></span></div>
      {:else}
        <button class="primary" onclick={capture} disabled={!target || !streaming}>● Capture {traces} traces</button>
      {/if}
      {#if captureError}<p class="error small">{captureError}</p>{/if}
      {#if info?.n}<p class="muted small">{info.n.toLocaleString()} traces, {info.samples.toLocaleString()} samples{info.aligned ? ' · aligned' : ''}{lastLatency ? ` · ${fmtTime(lastLatency)}/op` : ''}</p>{/if}
    </section>

    {#if info?.n}
      <section>
        <h3>Align</h3>
        <p class="muted small">Line every trace up on a feature, to undo jitter before the statistics.</p>
        <div class="row">
          <label>From <input type="number" min="0" bind:value={alignFrom} /></label>
          <label>To <input type="number" min="2" bind:value={alignTo} /></label>
        </div>
        <label>Max shift <input type="number" min="1" bind:value={maxShift} /></label>
        <div class="row">
          <button class="small" onclick={align} disabled={aligning}>{aligning ? 'Aligning…' : 'Align'}</button>
          <button class="ghost small" onclick={clearAlign} disabled={!info.aligned}>Reset</button>
        </div>
        {#if alignInfo}<p class="muted small">mean corr {alignInfo.mean_corr.toFixed(3)}{alignInfo.poor ? ` · ${alignInfo.poor} poor` : ''}</p>{/if}
      </section>
    {/if}
  </aside>

  <main>
    <div class="tabs">
      {#each [['traces', 'Traces'], ['ttest', 'T-test (TVLA)'], ['timing', 'Timing'], ['cpa', 'Correlation (CPA)'], ['export', 'Export']] as [k, label] (k)}
        <button class:on={view === k} onclick={() => (view = k as View)}>{label}</button>
      {/each}
      <span class="spacer"></span>
      {#if result && result.kind === 'ttest'}
        <span class="readout" class:leak={result.max_abs > 4.5}>max |t| {result.max_abs.toFixed(1)} at {fmtTime(result.at * result.dt)} · n {result.n0}+{result.n1}</span>
      {:else if result && result.kind === 'overview'}
        <span class="readout">{result.n.toLocaleString()} traces</span>
      {:else if view === 'timing' && timing}
        <span class="readout leak">longest: “{timing.guesses[timing.best]}” at {fmtTime(timing.mean[timing.best])}</span>
      {:else if view === 'cpa' && cpa}
        <span class="readout" class:leak={cpa.correct !== false}>byte {cpa.byte}: <b>0x{hex2(cpa.best)}</b> · r {cpa.best_corr.toFixed(3)} (2nd {cpa.second.toFixed(3)}){cpa.correct != null ? (cpa.correct ? ' ✓' : ' ✗') : ''}</span>
      {/if}
    </div>

    {#if view === 'cpa'}
      <div class="cpa-bar">
        <label>Key byte <input type="number" min="0" max="15" bind:value={cpaByte} /></label>
        <button class="small primary" onclick={runCpa} disabled={cpaBusy || !info?.n}>{cpaBusy && !recoverAll ? 'Working…' : 'Attack byte'}</button>
        <button class="small" onclick={recoverKey} disabled={cpaBusy || !info?.n}>{recoverAll ? `Recovering… ${recovered.filter((b) => b != null).length}/16` : 'Recover key (16 bytes)'}</button>
        <span class="spacer"></span>
        <span class="keyout" class:ok={keyCorrect} class:bad={knownKey && keyComplete && !keyCorrect}>
          key: <b>{recoveredHex}</b>{#if keyComplete}{knownKey ? (keyCorrect ? ' ✓ recovered' : ' ✗ wrong') : ' (done)'}{/if}
        </span>
      </div>
      <div class="chart" bind:this={chartEl}>
        {#if !cpa}
          <div class="empty">
            <p>Correlation power analysis.</p>
            <p class="muted">Capture <b>AES — random plaintext</b> (so the inputs vary), then <b>Attack byte</b> or <b>Recover key</b>. For each of the 256 guesses of a key byte, it correlates the Hamming weight of the first-round S-box output with every sample; the highest correlation is the byte.</p>
            <p class="muted small">The demo key is known, so a recovered byte is marked ✓ / ✗. On real traces nothing is known — the tallest bar, well clear of the rest, is the answer.</p>
          </div>
        {/if}
      </div>
    {:else if view === 'export'}
      <div class="export">
        <p>Save the campaign as a NumPy <code>.npz</code>: <code>traces</code> (float32, aligned), <code>textin</code>, <code>textout</code>, <code>group</code>, <code>width</code>, <code>meta.json</code>.</p>
        <p class="muted">Load it in Python (<code>numpy.load</code>), ChipWhisperer’s analyzer, Lascar or SCAred for CPA / DPA and deeper work.</p>
        <button class="primary" onclick={exportNpz} disabled={!info?.n}>Save {info?.n ?? 0} traces…</button>
      </div>
    {:else}
      <div class="chart" bind:this={chartEl}>
        {#if !info?.n}
          <div class="empty">
            <p>No traces yet.</p>
            <p class="muted">Pick a target and press <b>Capture</b>. The demo target leaks like a real microcontroller, so the views work with no hardware.</p>
            <p class="muted small">
              {#if view === 'traces'}Traces: the mean and ±σ over all traces, with a few drawn. Leakage shows as spread at the sensitive moments.
              {:else if view === 'ttest'}TVLA: Welch’s t per sample between fixed and random inputs. |t| over 4.5 (the dashed lines) means the power depends on the data — a leak.
              {:else}Timing: the operation’s duration per input. A password check that stops at the first wrong byte takes longer for a right guess.{/if}
            </p>
          </div>
        {/if}
      </div>
    {/if}
  </main>
</div>

<style>
  .sca {
    display: grid;
    grid-template-columns: 240px 1fr;
    grid-template-rows: auto 1fr;
    gap: 10px;
    height: 100%;
    min-height: 0;
    padding: 10px;
  }
  .disclaimer {
    grid-column: 1 / -1;
    font-size: 12px;
    line-height: 1.4;
    color: var(--text);
    background: color-mix(in srgb, var(--warn, #e0a030) 14%, transparent);
    border: 1px solid color-mix(in srgb, var(--warn, #e0a030) 45%, transparent);
    border-radius: var(--r);
    padding: 7px 10px;
  }
  .panels {
    display: flex;
    flex-direction: column;
    gap: 10px;
    overflow-y: auto;
    min-height: 0;
    grid-row: 2;
  }
  section {
    border: 1px solid var(--border);
    border-radius: var(--r);
    background: var(--panel);
    padding: 10px;
    display: flex;
    flex-direction: column;
    gap: 7px;
  }
  h3 {
    margin: 0;
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--muted);
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 3px;
    font-size: 12px;
    color: var(--muted);
  }
  .row {
    display: flex;
    gap: 6px;
  }
  .row label,
  .row select {
    flex: 1;
    min-width: 0;
  }
  input,
  select {
    width: 100%;
    padding: 3px 6px;
    font-size: 13px;
    box-sizing: border-box;
  }
  .mono {
    font-family: ui-monospace, monospace;
  }
  .seg {
    display: flex;
    gap: 2px;
  }
  .seg button {
    flex: 1;
  }
  .seg button.on {
    background: var(--accent);
    color: #fff;
    border-color: var(--accent);
  }
  .status {
    margin: 2px 0 0;
    font-size: 12px;
    color: var(--muted);
  }
  .status.ok {
    color: var(--ok);
  }
  .bar {
    height: 5px;
    border-radius: 3px;
    background: var(--border);
    overflow: hidden;
  }
  .bar span {
    display: block;
    height: 100%;
    background: var(--accent);
    transition: width 0.1s;
  }
  main {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    grid-row: 2;
    border: 1px solid var(--border);
    border-radius: var(--r);
    background: var(--panel);
  }
  .tabs {
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 6px 8px;
    border-bottom: 1px solid var(--border);
  }
  .tabs button {
    border: 0;
    border-radius: var(--r-sm);
    background: none;
    padding: 4px 10px;
    font-size: 13px;
    color: var(--muted);
  }
  .tabs button.on {
    color: var(--text);
    background: var(--bg);
  }
  .spacer {
    flex: 1;
  }
  .readout {
    font: 12px ui-monospace, monospace;
    color: var(--muted);
  }
  .readout.leak {
    color: var(--danger);
    font-weight: 600;
  }
.cpa-bar {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 8px;
    border-bottom: 1px solid var(--border);
    font-size: 12px;
  }
  .cpa-bar label {
    display: flex;
    align-items: center;
    gap: 4px;
    color: var(--muted);
  }
  .cpa-bar input {
    width: 48px;
    padding: 2px 4px;
  }
  .keyout {
    font: 12px ui-monospace, monospace;
    color: var(--muted);
  }
  .keyout b {
    color: var(--text);
    letter-spacing: 0.03em;
  }
  .keyout.ok,
  .keyout.ok b {
    color: var(--ok);
  }
  .keyout.bad,
  .keyout.bad b {
    color: var(--danger);
  }
  .chart {
    flex: 1;
    min-height: 0;
    position: relative;
  }
  .empty {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 6px;
    text-align: center;
    padding: 0 40px;
  }
  .empty p {
    margin: 0;
    max-width: 480px;
  }
  .export {
    padding: 20px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    align-items: flex-start;
  }
  .export button {
    margin-top: 6px;
  }
  code {
    font-family: ui-monospace, monospace;
    font-size: 0.92em;
    background: var(--bg);
    padding: 0 3px;
    border-radius: var(--r-sm);
  }
  .error {
    color: var(--danger);
  }
</style>
