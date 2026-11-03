<script lang="ts" module>
  import type { Signal } from './nbp';

  export interface AxisCfg {
    auto: boolean;
    /** SI units */
    min: number;
    max: number;
  }

  /** what a plot draws per pixel column: the mean line, the min..max band, or both */
  export type TraceMode = 'both' | 'mean' | 'range';

  export const SIGNAL_INFO: Record<Signal, { name: string; unit: string; color: string }> = {
    i: { name: 'Current', unit: 'A', color: '--trace-i' },
    v: { name: 'Voltage', unit: 'V', color: '--trace-v' },
    p: { name: 'Power', unit: 'W', color: '--trace-p' },
  };
</script>

<script lang="ts">
  // The scope screen: one plot per visible signal, stacked, sharing the time
  // axis (the view's span, 10 divisions labelled from its right edge) and
  // the crosshair. Every frame (at most ~30 / s) reduces the store once to
  // about one point per pixel column and hands it to all plots.
  //
  // Mouse: drag = select a span (goes to HOLD), wheel = time/div, shift+wheel
  // = pan, double-click = clear the selection.
  import { onMount } from 'svelte';
  import uPlot from 'uplot';
  import 'uplot/dist/uPlot.min.css';
  import type { Lod, PointStore } from './store';
  import { DIVS_X, DIVS_Y, fmtTime, type View } from './view.svelte';
  import { prefixFor } from './si';

  let {
    store,
    view,
    signals,
    axes,
    mode = 'both',
  }: {
    store: PointStore;
    view: View;
    signals: Signal[];
    axes: Record<Signal, AxisCfg>;
    mode?: TraceMode;
  } = $props();

  const MIN_FRAME_MS = 33;
  const SYNC_KEY = 'nebulosa-scope';

  let root: HTMLDivElement;
  let hosts: Record<string, HTMLDivElement> = $state({});
  let plots: { sig: Signal; u: uPlot }[] = [];
  let drawnKey = '';
  let lastDraw = 0;
  let span: [number, number] = [0, 1];
  let lod: Lod | null = null;
  let raf = 0;
  /** the selection's bracket over the plots, CSS px from the scope's left edge */
  let selBar = $state<{ left: number; width: number; clipL: boolean; clipR: boolean } | null>(null);
  let plotArea = $state({ left: 0, width: 0 });
  let readout = $state<{ t: number; v: Partial<Record<Signal, number | null>> } | null>(null);

  function cssVar(name: string) {
    return getComputedStyle(root).getPropertyValue(name).trim();
  }

  function yRange(sig: Signal) {
    return (_u: uPlot, lo: number | null, hi: number | null): uPlot.Range.MinMax => {
      const cfg = axes[sig];
      if (!cfg.auto) return [cfg.min, cfg.max];
      if (lo == null || hi == null || !isFinite(lo) || !isFinite(hi)) return [0, 1];
      const pad = Math.max((hi - lo) * 0.08, Math.abs(hi) * 1e-3, 1e-12);
      return [lo - pad, hi + pad];
    };
  }

  function divisions(n: number) {
    return (_u: uPlot, _i: number, min: number, max: number) =>
      Array.from({ length: n + 1 }, (_, k) => min + (k * (max - min)) / n);
  }

  /** tick labels with one prefix for the whole axis */
  function siValues(unit: string) {
    return (_u: uPlot, v: number[]) => {
      const mag = Math.max(...v.map(Math.abs));
      const p = prefixFor(mag);
      const step = v.length > 1 ? Math.abs(v[1] - v[0]) / p[0] : 1;
      const dec = Math.max(0, Math.min(4, -Math.floor(Math.log10(step || 1)) + 1));
      return v.map((x, k) => (k % 2 ? '' : `${(x / p[0]).toFixed(dec)} ${p[1]}${unit}`));
    };
  }

  function overlay(_first: boolean) {
    return (u: uPlot) => {
      const { ctx } = u;
      const { left, top, width, height } = u.bbox;
      const dpr = devicePixelRatio;
      ctx.save();
      ctx.strokeStyle = cssVar('--scope-axis');
      ctx.lineWidth = dpr;
      ctx.beginPath();
      ctx.moveTo(left + width / 2, top);
      ctx.lineTo(left + width / 2, top + height);
      ctx.moveTo(left, top + height / 2);
      ctx.lineTo(left + width, top + height / 2);
      ctx.stroke();

      const sel = view.selection;
      if (sel) {
        const x0 = u.valToPos(sel[0], 'x', true);
        const x1 = u.valToPos(sel[1], 'x', true);
        const a = Math.max(left, Math.min(x0, x1));
        const b = Math.min(left + width, Math.max(x0, x1));
        const col = cssVar('--sel');
        if (b > a) {
          ctx.fillStyle = col + '26';
          ctx.fillRect(a, top, b - a, height);
        }
        ctx.strokeStyle = col;
        ctx.lineWidth = 1.5 * dpr;
        for (const x of [x0, x1]) {
          if (x < left || x > left + width) continue;
          ctx.beginPath();
          ctx.moveTo(x, top);
          ctx.lineTo(x, top + height);
          ctx.stroke();
        }
      }
      ctx.restore();
    };
  }

  function build() {
    for (const p of plots) p.u.destroy();
    plots = [];
    const text = cssVar('--scope-text');
    const grid = { stroke: cssVar('--scope-grid'), width: 1, dash: [2, 4] };
    const axis = { stroke: text, grid, ticks: { show: false }, font: '11px ui-monospace, monospace' };
    const hidden = { stroke: 'transparent', width: 0, points: { show: false } };
    signals.forEach((sig, idx) => {
      const el = hosts[sig];
      if (!el) return;
      const last = idx === signals.length - 1;
      const col = cssVar(SIGNAL_INFO[sig].color);
      const u = new uPlot(
        {
          width: el.clientWidth,
          height: el.clientHeight,
          scales: { x: { time: false, auto: false }, y: { auto: true, range: yRange(sig) } },
          legend: { show: false },
          cursor: { drag: { x: true, y: false, setScale: false }, points: { show: false }, sync: { key: SYNC_KEY } },
          series: [
            {},
            mode === 'range' ? { stroke: col + 'aa', width: 1, points: { show: false } } : { ...hidden },
            mode === 'range' ? { stroke: col + 'aa', width: 1, points: { show: false } } : { ...hidden },
            mode === 'range' ? { ...hidden } : { stroke: col, width: 1.5, points: { show: false } },
          ],
          bands: mode === 'mean' ? [] : [{ series: [2, 1], fill: col + (mode === 'range' ? '55' : '30') }],
          axes: [
            {
              ...axis,
              splits: divisions(DIVS_X),
              size: last ? 28 : 6,
              values: (_u, v) => (last ? v.map((x, k) => (k % 2 ? '' : fmtTime(x - span[1], 2))) : v.map(() => '')),
            },
            { ...axis, size: 78, splits: divisions(DIVS_Y), values: siValues(SIGNAL_INFO[sig].unit), stroke: col },
          ],
          hooks: {
            draw: [overlay(idx === 0)],
            setSelect: [
              (u) => {
                if (u.select.width > 3) {
                  const a = u.posToVal(u.select.left, 'x');
                  const b = u.posToVal(u.select.left + u.select.width, 'x');
                  view.hold(store);
                  view.selection = [a, b];
                }
                u.setSelect({ left: 0, top: 0, width: 0, height: 0 }, false);
              },
            ],
            setCursor: [
              (u) => {
                const k = u.cursor.idx;
                if (k == null || u.cursor.left == null || u.cursor.left < 0 || !lod) {
                  readout = null;
                  return;
                }
                readout = {
                  t: lod.t[k] - span[1],
                  v: Object.fromEntries(signals.map((s) => [s, lod!.ch[s].avg[k]])),
                };
              },
            ],
          },
        },
        [[], [], [], []],
        el,
      );
      u.over.addEventListener('wheel', onWheel(u), { passive: false });
      u.over.addEventListener('dblclick', onDbl);
      plots.push({ sig, u });
    });
    drawnKey = '';
  }

  function onWheel(u: uPlot) {
    return (e: WheelEvent) => {
      e.preventDefault();
      if (e.shiftKey || Math.abs(e.deltaX) > Math.abs(e.deltaY)) {
        const d = e.shiftKey ? e.deltaY : e.deltaX;
        view.pan(store, Math.sign(d) * 0.1);
      } else {
        view.zoom(store, Math.sign(e.deltaY), u.posToVal(e.offsetX, 'x'));
      }
    };
  }

  function onDbl(e: MouseEvent) {
    e.preventDefault();
    view.selection = null;
  }

  function frame(now: number) {
    raf = requestAnimationFrame(frame);
    if (!plots.length || now - lastDraw < MIN_FRAME_MS) return;
    const sel = view.selection;
    const key = [
      view.live ? store.version : `${view.end}`,
      view.divIndex,
      store.tStart,
      sel ? sel.join() : '',
      signals.map((s) => `${axes[s].auto},${axes[s].min},${axes[s].max}`).join(';'),
      mode,
      plots.map((p) => `${p.u.width}x${p.u.height}`).join(),
    ].join('|');
    if (key === drawnKey) return;
    drawnKey = key;
    lastDraw = now;
    span = view.span(store);
    const bins = Math.max(50, Math.round(plots[0].u.bbox.width / devicePixelRatio));
    lod = store.lod(span[0], span[1], bins);
    for (const { sig, u } of plots) {
      const c = lod.ch[sig];
      u.batch(() => {
        u.setData([lod!.t, c.min, c.max, c.avg] as uPlot.AlignedData, false);
        u.setScale('x', { min: span[0], max: span[1] });
      });
    }
    updateSelBar();
  }

  function updateSelBar() {
    const u = plots[0]?.u;
    if (!u) return;
    const dpr = devicePixelRatio;
    const left = u.bbox.left / dpr;
    const width = u.bbox.width / dpr;
    if (left !== plotArea.left || width !== plotArea.width) plotArea = { left, width };
    const sel = view.selection;
    if (!sel) {
      selBar = null;
      return;
    }
    const x0 = left + u.valToPos(sel[0], 'x');
    const x1 = left + u.valToPos(sel[1], 'x');
    const a = Math.max(left, x0);
    const b = Math.min(left + width, x1);
    selBar = b > a ? { left: a, width: b - a, clipL: x0 < left, clipR: x1 > left + width } : null;
  }

  onMount(() => {
    raf = requestAnimationFrame(frame);
    const ro = new ResizeObserver(() => {
      for (const { sig, u } of plots) {
        const el = hosts[sig];
        if (el) u.setSize({ width: el.clientWidth, height: el.clientHeight });
      }
    });
    ro.observe(root);
    return () => {
      cancelAnimationFrame(raf);
      ro.disconnect();
      for (const p of plots) p.u.destroy();
    };
  });

  // rebuild when the set of signals changes (after the host divs exist)
  $effect(() => {
    const key = signals.join() + mode;
    void key;
    queueMicrotask(build);
  });
</script>

<div class="scope" bind:this={root}>
  <div class="markers">
    {#if view.selection && selBar}
      <div
        class="bracket"
        class:clipL={selBar.clipL}
        class:clipR={selBar.clipR}
        style="left: {selBar.left}px; width: {selBar.width}px"
      >
        <span>Δt {fmtTime(view.selection[1] - view.selection[0])}</span>
      </div>
    {:else if view.selection}
      <span class="hintbar" style="left: {plotArea.left}px">selection off screen</span>
    {:else}
      <span class="hintbar" style="left: {plotArea.left}px">drag across a plot to select a span</span>
    {/if}
    <span class="legend">
      {#if mode !== 'range'}<span class="key"><i class="line"></i>mean</span>{/if}
      {#if mode !== 'mean'}<span class="key"><i class="band"></i>min–max</span>{/if}
      <span class="muted">per pixel column</span>
    </span>
  </div>
  {#each signals as sig (sig)}
    <div class="row">
      <div class="host" bind:this={hosts[sig]}></div>
      <span class="name" style="color: var({SIGNAL_INFO[sig].color})">{SIGNAL_INFO[sig].name}</span>
      {#if readout && readout.v[sig] != null}
        <span class="val" style="color: var({SIGNAL_INFO[sig].color})">
          {(() => {
            const x = readout.v[sig]!;
            const p = prefixFor(x);
            return `${(x / p[0]).toFixed(3)} ${p[1]}${SIGNAL_INFO[sig].unit}`;
          })()}
        </span>
      {/if}
    </div>
  {:else}
    <div class="empty">No signals shown.</div>
  {/each}
  {#if readout}
    <span class="t">t {fmtTime(readout.t)}</span>
  {/if}
</div>

<style>
  .scope {
    position: relative;
    width: 100%;
    height: 100%;
    display: flex;
    flex-direction: column;
    background: var(--scope-bg);
    border: 1px solid var(--scope-border);
    border-radius: 6px;
    overflow: hidden;
  }
  .markers {
    position: relative;
    height: 24px;
    flex: none;
    border-bottom: 1px solid var(--scope-border);
    font: 12px ui-monospace, monospace;
    color: var(--scope-text);
  }
  .bracket {
    position: absolute;
    top: 3px;
    bottom: 3px;
    border: 1.5px solid var(--sel);
    border-top: 0;
    border-radius: 0 0 3px 3px;
    background: color-mix(in srgb, var(--sel) 18%, transparent);
    display: flex;
    align-items: center;
    justify-content: center;
    min-width: 2px;
    overflow: visible;
  }
  .bracket.clipL {
    border-left-style: dashed;
  }
  .bracket.clipR {
    border-right-style: dashed;
  }
  .bracket span {
    background: #000;
    color: #fff;
    border: 1px solid var(--sel);
    border-radius: 3px;
    padding: 0 6px;
    white-space: nowrap;
    font-weight: 600;
  }
  .hintbar {
    position: absolute;
    top: 4px;
    font-size: 11px;
    opacity: 0.7;
  }
  .legend {
    position: absolute;
    right: 10px;
    top: 4px;
    display: flex;
    gap: 10px;
    align-items: center;
    font-size: 11px;
  }
  .key {
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }
  .key i {
    display: inline-block;
    width: 16px;
  }
  .key .line {
    height: 2px;
    background: var(--scope-text);
  }
  .key .band {
    height: 9px;
    background: color-mix(in srgb, var(--scope-text) 35%, transparent);
  }
  .muted {
    opacity: 0.6;
  }
  .row {
    position: relative;
    flex: 1;
    min-height: 90px;
    border-bottom: 1px solid var(--scope-border);
  }
  .row:last-of-type {
    border-bottom: 0;
  }
  .host {
    position: absolute;
    inset: 0;
  }
  .name,
  .val {
    position: absolute;
    top: 4px;
    font: 600 11px ui-monospace, monospace;
    letter-spacing: 0.04em;
    pointer-events: none;
    padding: 1px 6px;
    border-radius: 3px;
    background: #000a;
  }
  .name {
    left: 84px;
    text-transform: uppercase;
  }
  .val {
    right: 12px;
    font-size: 12px;
  }
  .t {
    position: absolute;
    right: 12px;
    bottom: 32px;
    font: 12px ui-monospace, monospace;
    color: var(--scope-text);
    background: #000a;
    padding: 1px 6px;
    border-radius: 3px;
    pointer-events: none;
  }
  .empty {
    margin: auto;
    color: var(--scope-text);
  }
  .scope :global(.u-select) {
    background: color-mix(in srgb, var(--sel) 22%, transparent);
    border-left: 1px solid var(--sel);
    border-right: 1px solid var(--sel);
  }
  .scope :global(.u-cursor-x),
  .scope :global(.u-cursor-y) {
    border-color: #ffffff55;
  }
</style>
