<script lang="ts">
  // Analysis over a span (the active dual marker, else the screen), on every
  // raw sample: histogram, CDF / CCDF, spectrum (Welch PSD), worst window.
  // The work runs in the core (native, or WASM in the browser).
  import { onDestroy, onMount, tick, untrack } from 'svelte';
  import uPlot from 'uplot';
  import 'uplot/dist/uPlot.min.css';
  import type { Link } from './device';
  import type { AnalysisRequest, AnalysisResult, Signal } from './nbp';
  import { SIGNAL_INFO } from './Scope.svelte';
  import { fmtSI, logSplits, parseSI, prefixFor } from './si';
  import { fmtTime } from './view.svelte';

  type Tab = 'histogram' | 'cdf' | 'psd' | 'maxwindow';

  let {
    signals,
    link,
    span,
    spanLabel,
    onclose,
    onshow,
    initialTab = 'histogram',
    autorun = false,
  }: {
    /** the signals there are (the math channel once set) */
    signals: Signal[];
    link: Link | null;
    span: [number, number];
    spanLabel: string;
    onclose: () => void;
    /** puts a dual marker on [t0, t1] and shows it */
    onshow: (t0: number, t1: number) => void;
    /** the tab it opens on */
    initialTab?: Tab;
    /** run at once (the showcases) */
    autorun?: boolean;
  } = $props();

  let tab = $state<Tab>(untrack(() => initialTab));
  onMount(() => {
    if (autorun) runIt();
  });
  let source = $state<Signal>('i');
  let bins = $state(200);
  let logBins = $state(true);
  let nfft = $state(16384);
  let widthText = $state('1 ms');
  let busy = $state(false);
  let error = $state('');
  let result = $state.raw<AnalysisResult | null>(null);
  let took = $state(0);
  let ccdf = $state(true);

  let chartEl = $state<HTMLDivElement>();
  let chart: uPlot | undefined;

  const unit = $derived(SIGNAL_INFO[source].unit);

  function request(): AnalysisRequest | null {
    if (tab === 'histogram' || tab === 'cdf') return { kind: 'histogram', source, bins, log: logBins };
    if (tab === 'psd') return { kind: 'psd', source, nfft };
    const w = parseSI(widthText);
    if (!(w > 0)) {
      error = 'width: a time, e.g. 1 ms';
      return null;
    }
    return { kind: 'maxwindow', source, width: w };
  }

  // runs by itself: on open, and when the tab, signal or a setting changes
  // (a moment after the last change); the window width on Enter or Run
  let pending: ReturnType<typeof setTimeout> | undefined;
  let again = false;
  $effect(() => {
    void [tab, source, bins, logBins, nfft];
    clearTimeout(pending);
    pending = setTimeout(() => {
      if (busy) again = true;
      else runIt();
    }, 250);
  });
  onDestroy(() => clearTimeout(pending));

  async function runIt() {
    if (!link) return;
    const req = request();
    if (!req) return;
    busy = true;
    error = '';
    const t = performance.now();
    try {
      result = await link.analyze(span[0], span[1], req);
      took = performance.now() - t;
      if (result.kind === 'empty') error = result.reason;
      await tick();
      draw();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
      if (again) {
        again = false;
        runIt();
      }
    }
  }

  function css(n: string) {
    return getComputedStyle(document.documentElement).getPropertyValue(n).trim();
  }

  function axisFmt(u: string) {
    // uPlot thins crowded log ticks by passing null
    return (_: uPlot, v: (number | null)[]) => v.map((x) => (x == null ? '' : x === 0 ? '0' : fmtSI(x, u, 3)));
  }

  function draw() {
    chart?.destroy();
    chart = undefined;
    const r = result;
    if (!chartEl || !r || r.kind === 'empty' || r.kind === 'maxwindow') return;
    const col = css(SIGNAL_INFO[source].color);
    const axis = { stroke: css('--muted'), grid: { stroke: css('--grid') }, ticks: { show: false } };
    const W = chartEl.clientWidth;
    const H = chartEl.clientHeight;
    let data: uPlot.AlignedData;
    let opts: uPlot.Options;
    if (r.kind === 'histogram' && tab === 'histogram') {
      // a step per bin, from its left edge
      const x = r.edges.slice(0, -1);
      const total = r.counts.reduce((a, b) => a + b, 0) || 1;
      data = [x, r.counts.map((c) => c / total)];
      opts = {
        width: W,
        height: H,
        legend: { show: false },
        scales: { x: logBins ? { distr: 3, log: 10 } : { time: false }, y: { range: (_u, _lo, hi) => [0, (hi ?? 1) * 1.05] } },
        series: [{}, { stroke: col, fill: col + '55', width: 1, paths: uPlot.paths.stepped!({ align: 1 }), points: { show: false } }],
        axes: [
          { ...axis, values: axisFmt(unit), ...(logBins ? { splits: logSplits } : {}) },
          { ...axis, size: 60, values: (_u, v) => v.map((y) => `${(y * 100).toFixed(y < 0.01 ? 2 : 1)} %`) },
        ],
      };
    } else if (r.kind === 'histogram') {
      // CDF: fraction of samples at or below; CCDF: above
      const x = r.edges.slice(1);
      const total = r.counts.reduce((a, b) => a + b, 0) || 1;
      let acc = 0;
      const y = r.counts.map((c) => {
        acc += c;
        const cdf = acc / total;
        return ccdf ? Math.max(1 - cdf, 1e-9) : cdf;
      });
      data = [x, y];
      opts = {
        width: W,
        height: H,
        legend: { show: false },
        scales: {
          x: logBins ? { distr: 3, log: 10 } : { time: false },
          y: ccdf ? { distr: 3, log: 10, range: () => [Math.max(1e-6, 0.5 / total), 1] } : { range: () => [0, 1] },
        },
        series: [{}, { stroke: col, width: 1.5, points: { show: false } }],
        axes: [
          { ...axis, values: axisFmt(unit), ...(logBins ? { splits: logSplits } : {}) },
          {
            ...axis,
            size: 64,
            ...(ccdf ? { splits: logSplits } : {}),
            values: (_u, v) => v.map((p) => (p == null ? '' : `${+(p * 100).toPrecision(2)} %`)),
          },
        ],
      };
    } else {
      // PSD on log-log axes; skip DC
      const f = r.freq.slice(1);
      const p = r.psd.slice(1).map((v) => (v > 0 ? v : null));
      data = [f, p];
      opts = {
        width: W,
        height: H,
        legend: { show: false },
        scales: { x: { distr: 3, log: 10 }, y: { distr: 3, log: 10 } },
        series: [{}, { stroke: col, width: 1, points: { show: false } }],
        axes: [
          { ...axis, splits: logSplits, values: axisFmt('Hz') },
          { ...axis, size: 80, splits: logSplits, values: (_u, v) => v.map((y) => (y == null ? '' : `${y.toExponential(0)}`)) },
        ],
      };
    }
    chart = new uPlot(opts, data, chartEl);
  }

  // a result is per tab: switching tabs (or the CCDF toggle) redraws or clears
  let lastKind = '';
  $effect(() => {
    const k = tab === 'cdf' || tab === 'histogram' ? 'histogram' : tab;
    void ccdf;
    if (lastKind && lastKind !== k) result = null;
    lastKind = k;
    queueMicrotask(draw);
  });

  onDestroy(() => chart?.destroy());

  function fmtP(x: number) {
    const p = prefixFor(x);
    return `${(x / p[0]).toFixed(3)} ${p[1]}${unit}`;
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<div class="backdrop" role="presentation" onclick={onclose}>
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div class="dialog" onclick={(e) => e.stopPropagation()} role="dialog" aria-label="Analysis" data-tour="analysis" tabindex="-1">
    <header>
      <h2>Analysis</h2>
      <span class="muted">{spanLabel}</span>
      <button class="ghost small close" onclick={onclose}>✕</button>
    </header>

    <div class="tabs">
      {#each [['histogram', 'Histogram'], ['cdf', 'CDF / CCDF'], ['psd', 'Spectrum'], ['maxwindow', 'Max window']] as [k, label] (k)}
        <button class:on={tab === k} onclick={() => (tab = k as Tab)}>{label}</button>
      {/each}
    </div>

    <div class="controls">
      <div class="seg">
        {#each signals as sg (sg)}
          <button class:on={source === sg} onclick={() => (source = sg)}>{SIGNAL_INFO[sg].name}</button>
        {/each}
      </div>
      {#if tab === 'histogram' || tab === 'cdf'}
        <label class="inline">Bins <input type="number" min="10" max="5000" bind:value={bins} /></label>
        <label class="inline check"><input type="checkbox" bind:checked={logBins} /> log bins</label>
        {#if tab === 'cdf'}
          <div class="seg">
            <button class:on={!ccdf} onclick={() => (ccdf = false)} title="Fraction of samples at or below the value">CDF</button>
            <button class:on={ccdf} onclick={() => (ccdf = true)} title="Fraction of samples above the value">CCDF</button>
          </div>
        {/if}
      {:else if tab === 'psd'}
        <label class="inline">
          FFT size
          <select bind:value={nfft}>
            {#each [1024, 4096, 16384, 65536, 262144] as n (n)}<option value={n}>{n.toLocaleString()}</option>{/each}
          </select>
        </label>
      {:else}
        <label class="inline">Window <input bind:value={widthText} placeholder="1 ms" onkeydown={(e) => e.key === 'Enter' && runIt()} /></label>
      {/if}
      <button class="primary" onclick={runIt} disabled={busy || !link}>{busy ? 'Working…' : 'Run again'}</button>
    </div>

    {#if error}<p class="error">{error}</p>{/if}

    {#if result && result.kind === 'maxwindow'}
      <div class="mw">
        <p>
          Highest mean over {fmtTime(result.width)}: <b>{fmtP(result.mean)}</b>
          starting at {fmtTime(result.t - span[0])} into the span
          <span class="muted">({(result.mean / result.span_mean).toFixed(2)} × the span's mean, {fmtP(result.span_mean)})</span>
        </p>
        <button onclick={() => result && result.kind === 'maxwindow' && onshow(result.t, result.t + result.width)}>Show it (dual marker)</button>
      </div>
    {:else}
      <div class="chart" bind:this={chartEl}>
        {#if !result}<p class="muted hint">{busy ? "Working… every raw sample in the span." : "Every raw sample in the span is used."}</p>{/if}
      </div>
    {/if}

    {#if result && result.kind === 'histogram'}
      <div class="facts">
        <span>n {result.n.toLocaleString()}{result.excluded ? ` (${result.excluded.toLocaleString()} ≤ 0 left out)` : ''}</span>
        <span>mean {fmtP(result.mean)}</span>
        <span>σ {fmtP(result.std)}</span>
        <span>min {fmtP(result.min)}</span>
        <span>max {fmtP(result.max)}</span>
        {#each result.percentiles as [p, v] (p)}<span>p{Math.round(p * 100)} {fmtP(v)}</span>{/each}
      </div>
    {:else if result && result.kind === 'psd'}
      <div class="facts">
        <span>{result.segments} segments</span>
        <span>resolution {fmtSI(result.resolution, 'Hz', 3)}</span>
        <span>rms (AC) {fmtP(result.rms)}</span>
        <span class="muted">PSD in {unit}²/Hz</span>
      </div>
    {/if}
    {#if result && result.kind !== 'empty'}<small class="muted">{took.toFixed(0)} ms</small>{/if}
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: #0008;
    display: grid;
    place-items: center;
    z-index: 50;
  }
  .dialog {
    width: min(1000px, 92vw);
    height: min(680px, 88vh);
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: var(--r-lg);
    padding: 14px 18px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  header {
    display: flex;
    align-items: baseline;
    gap: 12px;
  }
  h2 {
    margin: 0;
    font-size: 16px;
  }
  .close {
    margin-left: auto;
  }
  .tabs {
    display: flex;
    gap: 4px;
    border-bottom: 1px solid var(--border);
  }
  .tabs button {
    border: 0;
    border-bottom: 2px solid transparent;
    border-radius: 0;
    background: transparent;
  }
  .tabs button.on {
    border-bottom-color: var(--accent);
    font-weight: 600;
  }
  .controls {
    display: flex;
    align-items: center;
    gap: 12px;
    flex-wrap: wrap;
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
    padding: 4px 10px;
    font-size: 12px;
  }
  .seg button.on {
    background: var(--accent);
    color: #fff;
  }
  label.inline {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 13px;
  }
  label.inline input:not([type='checkbox']),
  label.inline select {
    width: 110px;
  }
  .chart {
    flex: 1;
    min-height: 0;
    position: relative;
  }
  .hint {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
  }
  .facts {
    display: flex;
    flex-wrap: wrap;
    gap: 6px 16px;
    font: 12px ui-monospace, monospace;
    font-variant-numeric: tabular-nums;
  }
  .mw {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 10px;
    align-items: flex-start;
  }
  .error {
    color: var(--danger);
    margin: 0;
  }
  .muted {
    color: var(--muted);
  }
  button.small {
    padding: 2px 8px;
    font-size: 12px;
  }
</style>
