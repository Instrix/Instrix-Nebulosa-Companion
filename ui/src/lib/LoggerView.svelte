<script lang="ts" module>
  /** what the logger shows: following the whole session (or its last `last` s), or a fixed span */
  export interface LogView {
    follow: boolean;
    /** follow: seconds back from the newest (0: the whole session) */
    last: number;
    /** fixed span, FPGA time s */
    t0: number;
    t1: number;
  }
</script>

<script lang="ts">
  // The logger: the session log on a wall-clock axis, one plot per signal
  // (min–max band and mean per interval). Wheel: zoom, shift+wheel: pan,
  // drag: zoom to it, double-click: the whole session, click: open that
  // moment in the scope (when its samples are still in memory).
  import { onMount } from 'svelte';
  import uPlot from 'uplot';
  import 'uplot/dist/uPlot.min.css';
  import type { Signal } from './nbp';
  import { SIGNAL_INFO, type AxisCfg, type TraceCfg } from './Scope.svelte';
  import type { Lod, SampleStore } from './store';
  import { fmtSI, logSplits, prefixFor } from './si';
  import { fmtTime } from './view.svelte';
  import { drag as dragAct, store as save } from './drag';
  import HoverTip, { type TipRow } from './HoverTip.svelte';

  let {
    store,
    session,
    signals,
    axes,
    trace,
    wallOffset,
    lv = $bindable(),
    onopen,
    intervals = [],
    oninterval,
    onreset,
  }: {
    store: SampleStore;
    /** the session log's state (reactive) */
    session: { has_data: boolean; t_start: number; t_end: number; interval: number };
    signals: Signal[];
    axes: Record<Signal, AxisCfg>;
    trace: TraceCfg;
    /** unix time − FPGA time, s */
    wallOffset: number;
    lv: LogView;
    /** open FPGA time t in the scope */
    onopen: (t: number) => void;
    /** the log intervals to choose from, and a choice (it starts the session log over) */
    intervals?: number[];
    oninterval?: (s: number) => void;
    /** the Reset view button */
    onreset?: () => void;
  } = $props();

  const MIN_FRAME_MS = 200;
  let root: HTMLDivElement;
  let hosts: Record<string, HTMLDivElement> = $state({});
  let plots: { sig: Signal; u: uPlot }[] = [];
  let lod: Lod | null = null;
  let drawnKey = '';
  let lastDraw = 0;
  let inflight = false;
  let raf = 0;
  let readout = $state<{ t: number; v: Partial<Record<Signal, { avg: number; min: number; max: number; sd: number } | null>> } | null>(null);
  let plotsEl: HTMLDivElement;
  /** the mouse, px in the plots, for the hover box */
  let hover = $state<{ x: number; y: number; w: number; h: number } | null>(null);
  function onHover(e: PointerEvent) {
    const r = plotsEl.getBoundingClientRect();
    hover = { x: e.clientX - r.left, y: e.clientY - r.top, w: r.width, h: r.height };
  }
  const tipRows = $derived<TipRow[]>(
    readout
      ? signals.flatMap((s) => {
          const r = readout!.v[s];
          return r ? [{ name: SIGNAL_INFO[s].name, color: SIGNAL_INFO[s].color, unit: SIGNAL_INFO[s].unit, ...r }] : [];
        })
      : [],
  );
  /** the span on screen, FPGA time */
  let span = $state<[number, number]>([0, 1]);
  // ---- plot heights: the splitters between the plots (as in the scope) ----
  const WEIGHTS_KEY = 'nebulosa.loggerWeights';
  const MIN_ROW = 60;
  function loadWeights(): Record<Signal, number> {
    try {
      const w = JSON.parse(localStorage.getItem(WEIGHTS_KEY) ?? 'null');
      if (w && w.i > 0 && w.v > 0 && w.p > 0) return { m: 1, ...w };
    } catch {
      /* storage unavailable */
    }
    return { i: 1, v: 1, p: 1, m: 1 };
  }
  let weights = $state<Record<Signal, number>>(loadWeights());
  let rowEls: Record<string, HTMLDivElement> = $state({});
  function rowSplit(a: Signal, b: Signal) {
    let ha = 0, hb = 0, wa = 0, wb = 0;
    return {
      start: () => {
        ha = rowEls[a].clientHeight;
        hb = rowEls[b].clientHeight;
        wa = weights[a];
        wb = weights[b];
      },
      move: (_dx: number, dy: number) => {
        const d = Math.max(MIN_ROW - ha, Math.min(hb - MIN_ROW, dy));
        weights[a] = ((wa + wb) * (ha + d)) / (ha + hb);
        weights[b] = ((wa + wb) * (hb - d)) / (ha + hb);
      },
      end: () => save(WEIGHTS_KEY, $state.snapshot(weights)),
    };
  }
  function equalize() {
    weights = { i: 1, v: 1, p: 1, m: 1 };
    save(WEIGHTS_KEY, weights);
  }

  /** the plots at equal heights again */
  export function resetLayout() {
    equalize();
  }

  /** the time each block on screen averages */
  let avgWin = $state(0.1);
  /** the blocks are single samples (zoomed in on the sample buffer) */
  let single = $state(false);
  /** drawn from the sample buffer, not the session log */
  let buffered = $state(false);

  function cssVar(n: string) {
    return getComputedStyle(root ?? document.documentElement).getPropertyValue(n).trim();
  }

  /** the span lv describes, now */
  function wanted(): [number, number] {
    const s = store.session;
    if (!lv.follow) return [lv.t0, lv.t1];
    const end = s.t_end;
    const start = lv.last > 0 ? Math.max(s.t_start, end - lv.last) : s.t_start;
    return end > start ? [start, end] : [end - 1, end];
  }

  function yRange(sig: Signal) {
    return (_u: uPlot, lo: number | null, hi: number | null): uPlot.Range.MinMax => {
      const cfg = axes[sig];
      if (cfg.log) {
        if (!cfg.auto) return [Math.max(cfg.min, 1e-15), Math.max(cfg.max, 2e-15)];
        if (lo == null || hi == null || !(hi > 0)) return [1e-9, 1];
        return [Math.max(lo, hi * 1e-9, 1e-15) / 1.5, hi * 1.5];
      }
      if (!cfg.auto) return [cfg.min, cfg.max];
      if (lo == null || hi == null || !isFinite(lo) || !isFinite(hi)) return [0, 1];
      const pad = Math.max((hi - lo) * 0.08, Math.abs(hi) * 1e-3, 1e-12);
      return [lo - pad, hi + pad];
    };
  }

  function siValues(unit: string) {
    return (_u: uPlot, v: (number | null)[]) => {
      const nums = v.filter((x): x is number => x != null);
      const p = prefixFor(Math.max(0, ...nums.map(Math.abs)));
      return v.map((x) => (x == null ? '' : `${+(x / p[0]).toPrecision(3)} ${p[1]}${unit}`));
    };
  }

  const steps = uPlot.paths.stepped!({ align: 1 });

  /** the rows with one more at the end of the last interval, so its step has a width */
  function closeSteps(t: number[], cols: (number | null)[][], width: number) {
    const n = t.length;
    if (!n || cols[0][n - 1] == null) return;
    t.push(t[n - 1] + width);
    for (const c of cols) c.push(c[n - 1]);
  }

  function build() {
    for (const p of plots) p.u.destroy();
    plots = [];
    const text = cssVar('--scope-text');
    const axis = { stroke: text, grid: { stroke: cssVar('--scope-grid'), width: 1, dash: [2, 4] }, ticks: { show: false }, font: '11px ui-monospace, monospace' };
    const hidden = { stroke: 'transparent', width: 0, points: { show: false } };
    signals.forEach((sig, idx) => {
      const el = hosts[sig];
      if (!el) return;
      const last = idx === signals.length - 1;
      const col = cssVar(SIGNAL_INFO[sig].color);
      const log = axes[sig].log;
      const bands: uPlot.Band[] = [];
      if (trace.range) bands.push({ series: [2, 1], fill: col + '40' });
      if (trace.sigma) bands.push({ series: [4, 3], fill: col + '50' });
      const u = new uPlot(
        {
          width: el.clientWidth,
          height: el.clientHeight,
          // wall-clock seconds: uPlot draws dates and times
          scales: { x: { time: true, auto: false }, y: { auto: true, range: yRange(sig), ...(log ? { distr: 3, log: 10 } : {}) } },
          legend: { show: false },
          // room for the top and bottom y labels, so they stay off the neighbouring plots
          padding: [12, 8, last ? 0 : 12, 0],
          cursor: { drag: { x: true, y: false, setScale: false }, points: { show: false }, sync: { key: 'nebulosa-logger' } },
          // each logged interval is a block over its time (min..max band, the mean as a flat step), not a point
          series: [
            {},
            { ...hidden, paths: steps },
            { ...hidden, paths: steps },
            { ...hidden, paths: steps },
            { ...hidden, paths: steps },
            trace.mean ? { stroke: col, width: 1.25, paths: steps, points: { show: false } } : { ...hidden },
          ],
          bands,
          axes: [
            { ...axis, size: last ? 40 : 6, ...(last ? {} : { values: (_u: uPlot, v: number[]) => v.map(() => '') }) },
            { ...axis, size: 78, ...(log ? { splits: logSplits } : {}), values: siValues(SIGNAL_INFO[sig].unit), stroke: col },
          ],
          hooks: {
            setSelect: [
              (u) => {
                if (u.select.width > 3) {
                  const a = u.posToVal(u.select.left, 'x') - wallOffset;
                  const b = u.posToVal(u.select.left + u.select.width, 'x') - wallOffset;
                  lv = { follow: false, last: lv.last, t0: a, t1: b };
                }
                u.setSelect({ left: 0, top: 0, width: 0, height: 0 }, false);
              },
            ],
            setCursor: [
              (u) => {
                if (u.cursor.idx == null || u.cursor.left == null || u.cursor.left < 0 || !lod) {
                  readout = null;
                  return;
                }
                const L = lod;
                // the interval under the cursor: the last one starting at or before it
                const at = u.posToVal(u.cursor.left, 'x') - wallOffset;
                let k = 0;
                while (k + 1 < L.t.length && L.t[k + 1] <= at) k++;
                readout = {
                  t: L.t[k],
                  v: Object.fromEntries(
                    signals.map((s) => {
                      const c = L.ch[s];
                      const avg = c.avg[k];
                      return [s, avg == null ? null : { avg, min: c.min[k]!, max: c.max[k]!, sd: c.sdHi[k] != null ? c.sdHi[k]! - avg : 0 }];
                    }),
                  ),
                };
              },
            ],
          },
        },
        [[], [], [], [], [], []],
        el,
      );
      u.over.addEventListener('wheel', onWheel(u), { passive: false });
      u.over.addEventListener('dblclick', () => (lv = { follow: true, last: 0, t0: 0, t1: 0 }));
      u.over.addEventListener('mousedown', onDown(u));
      plots.push({ sig, u });
    });
    drawnKey = '';
  }

  function onWheel(u: uPlot) {
    return (e: WheelEvent) => {
      e.preventDefault();
      const [a, b] = span;
      const w = b - a;
      if (e.shiftKey || Math.abs(e.deltaX) > Math.abs(e.deltaY)) {
        const d = Math.sign(e.shiftKey ? e.deltaY : e.deltaX) * w * 0.15;
        lv = { follow: false, last: lv.last, t0: a + d, t1: b + d };
      } else {
        const at = u.posToVal(e.offsetX, 'x') - wallOffset;
        const f = e.deltaY > 0 ? 1.4 : 1 / 1.4;
        // down to a few samples (the buffer has them for recent data)
        const nw = Math.max(w * f, 2e-5);
        const frac = (at - a) / w;
        lv = { follow: false, last: lv.last, t0: at - frac * nw, t1: at + (1 - frac) * nw };
      }
    };
  }

  /** a click (no drag): open that moment in the scope */
  function onDown(u: uPlot) {
    return (e: MouseEvent) => {
      const x0 = e.clientX;
      const up = (ev: MouseEvent) => {
        window.removeEventListener('mouseup', up);
        if (Math.abs(ev.clientX - x0) < 3 && ev.button === 0) onopen(u.posToVal(e.offsetX, 'x') - wallOffset);
      };
      window.addEventListener('mouseup', up);
    };
  }

  function frame(now: number) {
    raf = requestAnimationFrame(frame);
    if (!plots.length || inflight || now - lastDraw < MIN_FRAME_MS) return;
    const want = wanted();
    const key = [
      lv.follow ? store.session.version : '',
      want.join(),
      signals.map((s) => `${axes[s].auto},${axes[s].min},${axes[s].max},${axes[s].log}`).join(';'),
      plots.map((p) => `${p.u.width}x${p.u.height}`).join(),
      wallOffset,
    ].join('|');
    if (key === drawnKey) return;
    drawnKey = key;
    lastDraw = now;
    inflight = true;
    const bins = Math.max(50, Math.round(plots[0].u.bbox.width / devicePixelRatio));
    // zoomed in past the log interval on data the sample buffer still holds: draw from the buffer,
    // down to single samples; else from the session log
    const iv = store.session.interval;
    const fromBuf = (want[1] - want[0]) / bins < iv && want[0] >= store.tStart && want[1] <= store.tEnd;
    (fromBuf ? store.lod(want[0], want[1], bins) : store.sessionLod(want[0], want[1], bins))
      .then((l) => {
        lod = l;
        span = want;
        // the last block is one interval (or one bin when zoomed out) wide
        // zoomed out, several log intervals merge into one block; from the buffer, a block is a column (or one sample)
        const col = (want[1] - want[0]) / bins;
        const width = fromBuf
          ? l.raw && l.t.length > 1 ? (l.t[l.t.length - 1] - l.t[0]) / (l.t.length - 1) : col
          : l.raw ? iv : Math.max(iv, Math.ceil(col / iv) * iv);
        avgWin = width;
        single = fromBuf && l.raw;
        buffered = fromBuf;
        for (const { sig, u } of plots) {
          const c = l.ch[sig];
          const pos = (a: (number | null)[]) => (axes[sig].log ? a.map((x) => (x != null && x > 0 ? x : null)) : [...a]);
          const t = l.t.map((x) => x + wallOffset);
          const cols = [pos(c.min), pos(c.max), pos(c.sdLo), pos(c.sdHi), pos(c.avg)];
          closeSteps(t, cols, width);
          u.batch(() => {
            u.setData([t, ...cols] as uPlot.AlignedData, false);
            u.setScale('x', { min: want[0] + wallOffset, max: want[1] + wallOffset });
          });
        }
      })
      .catch((e) => console.warn('session lod', e))
      .finally(() => (inflight = false));
  }

  const ro = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(() => {
    for (const { sig, u } of plots) {
      const el = hosts[sig];
      if (el && (el.clientWidth !== u.width || el.clientHeight !== u.height)) u.setSize({ width: el.clientWidth, height: el.clientHeight });
    }
  });

  onMount(() => {
    raf = requestAnimationFrame(frame);
    ro?.observe(root);
    return () => {
      cancelAnimationFrame(raf);
      ro?.disconnect();
      for (const p of plots) p.u.destroy();
    };
  });

  $effect(() => {
    const key = signals.join() + signals.map((s) => axes[s].log).join() + trace.mean + trace.range + trace.sigma;
    void key;
    queueMicrotask(() => {
      build();
      for (const s of signals) if (hosts[s]) ro?.observe(hosts[s]);
    });
  });

  const LAST = [
    [0, 'All'],
    [60, '1 min'],
    [600, '10 min'],
    [3600, '1 h'],
    [6 * 3600, '6 h'],
    [86400, '1 day'],
  ] as const;

  function fmtClock(t: number) {
    return new Date((t + wallOffset) * 1000).toLocaleString();
  }

  /** wall-clock time with the second's fraction, to the precision the blocks on screen need */
  function fmtClockFine(t: number, step: number) {
    const u = t + wallOffset;
    const d = new Date(Math.floor(u) * 1000);
    const digits = Math.min(7, Math.max(0, Math.ceil(-Math.log10(step)) + 1));
    const frac = digits ? (u - Math.floor(u)).toFixed(digits).slice(1) : '';
    return `${d.toLocaleDateString()} ${d.toLocaleTimeString([], { hour12: false })}${frac}`;
  }
</script>

<div class="logger" bind:this={root}>
  <div class="bar">
    <button class="badge" class:live={lv.follow} onclick={() => (lv = { ...lv, follow: true })} title="Follow the newest entries">
      {lv.follow ? 'FOLLOW' : 'PAUSED'}
    </button>
    <div class="seg">
      {#each LAST as [s, label] (s)}
        <button class:on={lv.follow && lv.last === s} onclick={() => (lv = { follow: true, last: s, t0: 0, t1: 0 })}>{label}</button>
      {/each}
    </div>
    <span class="mono muted">{fmtTime(span[1] - span[0])} shown</span>
    <span
      class="avgwin"
      title="Each block on the plots is the average of this much time; the shaded band spans its lowest to highest sample. The log interval is the shortest block the session log keeps; zoom in on recent data (still in the sample buffer) to see single samples."
    >
      each block =
      {#if single}<b>1 sample</b> ({fmtTime(avgWin)}){:else}<b>{fmtTime(avgWin)}</b> average{/if}
      {#if buffered}<span class="src">from the sample buffer</span>{/if}
    </span>
    {#if intervals.length && oninterval}
      <label class="ivsel" title="The session log's interval: one min/max/mean/σ entry per interval. Changing it starts the log over.">
        log interval
        <select value={session.interval} onchange={(e) => oninterval(Number((e.currentTarget as HTMLSelectElement).value))}>
          {#each intervals as s (s)}<option value={s}>{fmtTime(s)}</option>{/each}
        </select>
      </label>
    {/if}
    {#if session.has_data}
      <span class="mono muted">since {fmtClock(session.t_start)}</span>
    {/if}
    <button class="ghost small reset" onclick={() => onreset?.()} title="Plots back as they first were: equal heights, axes on auto, all of the session, following">Reset view</button>
    <span class="hint muted">line: average · band: min–max · wheel: zoom (in to single samples on recent data) · shift+wheel: pan · drag: zoom in · double-click: all · click: open in scope</span>
  </div>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="plots" bind:this={plotsEl} onpointermove={onHover} onpointerleave={() => (hover = null)}>
    {#each signals as sig, k (sig)}
      {@const info = SIGNAL_INFO[sig]}
      {#if k > 0}
        <div
          class="rsplit"
          role="separator"
          aria-orientation="horizontal"
          title="Drag to resize the plots; double-click: equal heights"
          use:dragAct={rowSplit(signals[k - 1], sig)}
          ondblclick={equalize}
        ></div>
      {/if}
      <div class="row" bind:this={rowEls[sig]} style="flex: {weights[sig]} 1 0px">
        <div class="host" bind:this={hosts[sig]}></div>
        <span class="name" style="color: var({info.color})">{info.name}</span>
      </div>
    {:else}
      <div class="empty">No signals shown.</div>
    {/each}
    {#if !session.has_data}
      <div class="empty overlay">The session log fills as data arrives: one entry per {fmtTime(session.interval)}.</div>
    {/if}
    {#if readout && hover && tipRows.length}
      <HoverTip
        x={hover.x}
        y={hover.y}
        boxW={hover.w}
        boxH={hover.h}
        title={fmtClockFine(readout.t, avgWin)}
        note={single ? '1 sample' : `${fmtTime(avgWin)} average`}
        rows={tipRows}
      />
    {/if}
  </div>
</div>

<style>
  .logger {
    height: 100%;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 12px;
    font-size: 12px;
    flex-wrap: wrap;
  }
  .badge {
    font: 600 11px ui-monospace, monospace;
    letter-spacing: 0.08em;
    padding: 3px 8px;
    border-radius: var(--r-sm);
    border: 1px solid var(--hold);
    background: var(--hold);
    color: #000;
  }
  .badge.live {
    background: transparent;
    border-color: var(--live);
    color: var(--live);
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
    padding: 3px 9px;
    font-size: 12px;
  }
  .seg button.on {
    background: var(--accent);
    color: #fff;
  }
  .mono {
    font-family: ui-monospace, monospace;
  }
  .muted {
    color: var(--muted);
  }
  .src {
    margin-left: 6px;
    color: var(--accent);
  }
  .ivsel {
    display: inline-flex;
    white-space: nowrap;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: var(--muted);
  }
  .ivsel select {
    padding: 1px 4px;
    font-size: 12px;
  }
  .avgwin {
    font: 12px ui-monospace, monospace;
    padding: 1px 8px;
    border: 1px solid var(--border);
    border-radius: var(--r-sm);
  }
  .hint {
    margin-left: auto;
    font-size: 11px;
  }
  .plots {
    position: relative;
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    background: var(--scope-bg);
    border: 1px solid var(--scope-border);
    border-radius: var(--r);
    overflow: hidden;
    color: var(--scope-text);
  }
  .row {
    position: relative;
    min-height: 60px;
  }
  .rsplit {
    flex: none;
    height: 10px;
    position: relative;
    z-index: 3;
    cursor: row-resize;
    touch-action: none;
  }
  .rsplit::after {
    content: '';
    position: absolute;
    left: 0;
    right: 0;
    top: 4px;
    border-top: 1px solid var(--scope-border);
  }
  .rsplit:hover::after,
  .rsplit:active::after {
    border-top: 2px solid var(--accent);
  }
  .host {
    position: absolute;
    inset: 0;
  }
  .name {
    position: absolute;
    font: 600 11px ui-monospace, monospace;
    padding: 1px 6px;
    border-radius: var(--r-sm);
    background: #000c;
    pointer-events: none;
  }
  .name {
    top: 4px;
    left: 84px;
    text-transform: uppercase;
  }
  .empty {
    margin: auto;
    color: var(--scope-text);
  }
  .overlay {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    pointer-events: none;
  }
  .plots :global(.u-select) {
    background: color-mix(in srgb, var(--accent) 22%, transparent);
  }
</style>
