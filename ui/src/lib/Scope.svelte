<script lang="ts" module>
  import type { Signal } from './nbp';

  export interface AxisCfg {
    auto: boolean;
    /** SI units */
    min: number;
    max: number;
    log: boolean;
  }

  /** what a plot draws per pixel column */
  export interface TraceCfg {
    mean: boolean;
    range: boolean;
    sigma: boolean;
  }

  export const SIGNAL_INFO: Record<Signal, { name: string; sym: string; unit: string; color: string; integral?: string }> = {
    i: { name: 'Current', sym: 'I', unit: 'A', color: '--trace-i', integral: 'C' },
    v: { name: 'Voltage', sym: 'V', unit: 'V', color: '--trace-v' },
    p: { name: 'Power', sym: 'P', unit: 'W', color: '--trace-p', integral: 'J' },
  };
</script>

<script lang="ts">
  // The scope screen: one plot per visible signal, stacked, sharing the time
  // axis (the view's span, 10 divisions labelled from its right edge), the
  // crosshair and the markers. Every frame (at most ~30 / s) reduces the
  // store once to about one point per pixel column for all plots.
  //
  // Per column: the mean line, the min–max band, the ±σ band (each optional).
  // Red ticks: samples missing (stream gaps).
  //
  // Mouse on a plot: drag = new dual marker (goes to HOLD), double-click =
  // single marker, wheel = time/div, shift+wheel = pan. In the marker bar:
  // drag a marker's edges or body, click to make it active, ✕ to delete.
  import { onMount } from 'svelte';
  import uPlot from 'uplot';
  import 'uplot/dist/uPlot.min.css';
  import type { Lod, PointValue, SampleStore } from './store';
  import { DIVS_X, DIVS_Y, fmtTime, markerColor, markerName, type Marker, type View } from './view.svelte';
  import { fmtSI, prefixFor } from './si';
  import type { TriggerState } from './trigger.svelte';

  let {
    store,
    view,
    signals,
    axes,
    trace,
    trig,
  }: {
    store: SampleStore;
    view: View;
    signals: Signal[];
    axes: Record<Signal, AxisCfg>;
    trace: TraceCfg;
    trig: TriggerState;
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

  /** geometry of the plot area, CSS px from the scope's left edge */
  let area = $state({ left: 0, width: 0 });
  /** markers on screen: x positions, CSS px from the scope's left edge */
  let geo = $state<{ m: Marker; x0: number; x1: number | null; c0: boolean; c1: boolean }[]>([]);
  /** per-plot labels: the active dual marker's statistics, single markers' values */
  let dualStats = $state<{ m: Marker; x: number; vals: Record<Signal, { mean: number; integral: number }> } | null>(null);
  /** the trigger's x in the marker bar, or null off screen */
  let trigX = $state<number | null>(null);
  let singleVals = $state<{ m: Marker; x: number; v: PointValue }[]>([]);
  let readout = $state<{ t: number; v: Partial<Record<Signal, { avg: number; min: number; max: number } | null>> } | null>(
    null,
  );

  function cssVar(name: string) {
    return getComputedStyle(root).getPropertyValue(name).trim();
  }

  function yRange(sig: Signal) {
    return (_u: uPlot, lo: number | null, hi: number | null): uPlot.Range.MinMax => {
      const cfg = axes[sig];
      if (cfg.log) {
        if (!cfg.auto) return [Math.max(cfg.min, 1e-15), Math.max(cfg.max, 2e-15)];
        if (lo == null || hi == null || !(hi > 0)) return [1e-9, 1];
        const a = Math.max(lo, hi * 1e-9, 1e-15);
        return [a / 1.5, hi * 1.5];
      }
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

  /**
   * Log axis ticks: every decade, plus 2 and 5 within it when there are few
   * decades. (Our own: uPlot's log splitter can loop forever when its
   * increment comes out as a non-power of ten.)
   */
  function logSplits(_u: uPlot, _i: number, min: number, max: number) {
    if (!(min > 0) || !(max > min)) return [];
    const lo = Math.floor(Math.log10(min));
    const hi = Math.ceil(Math.log10(max));
    const mults = hi - lo <= 3 ? [1, 2, 5] : [1];
    const out: number[] = [];
    for (let e = lo; e <= hi && out.length < 64; e++)
      for (const k of mults) {
        const v = k * 10 ** e;
        if (v >= min && v <= max) out.push(v);
      }
    // within one decade there may be no such tick: evenly spaced ones then
    return out.length >= 2 ? out : divisions(4)(_u, _i, min, max);
  }

  /** tick labels: one prefix for a linear axis; per tick on a log axis */
  function siValues(unit: string, log: boolean) {
    return (_u: uPlot, v: number[]) => {
      if (log) return v.map((x) => fmtSI(x, unit, 1));
      const mag = Math.max(...v.map(Math.abs));
      const p = prefixFor(mag);
      const step = v.length > 1 ? Math.abs(v[1] - v[0]) / p[0] : 1;
      const dec = Math.max(0, Math.min(4, -Math.floor(Math.log10(step || 1)) + 1));
      return v.map((x, k) => (k % 2 ? '' : `${(x / p[0]).toFixed(dec)} ${p[1]}${unit}`));
    };
  }

  function overlay(sig: Signal) {
    return (u: uPlot) => {
      const { ctx } = u;
      const { left, top, width, height } = u.bbox;
      const dpr = devicePixelRatio;
      const xOf = (t: number) => u.valToPos(t, 'x', true);
      const inside = (x: number) => x >= left && x <= left + width;
      ctx.save();
      // center cross (linear axes only: the middle of a log axis means nothing)
      ctx.strokeStyle = cssVar('--scope-axis');
      ctx.lineWidth = dpr;
      ctx.beginPath();
      ctx.moveTo(left + width / 2, top);
      ctx.lineTo(left + width / 2, top + height);
      if (!axes[sig].log) {
        ctx.moveTo(left, top + height / 2);
        ctx.lineTo(left + width, top + height / 2);
      }
      ctx.stroke();

      // gaps: missing samples
      if (lod?.gaps.length) {
        ctx.strokeStyle = cssVar('--danger');
        ctx.fillStyle = cssVar('--danger');
        for (const t of lod.gaps) {
          const x = xOf(t);
          if (!inside(x)) continue;
          ctx.globalAlpha = 0.35;
          ctx.setLineDash([2 * dpr, 3 * dpr]);
          ctx.beginPath();
          ctx.moveTo(x, top);
          ctx.lineTo(x, top + height);
          ctx.stroke();
          ctx.globalAlpha = 1;
          ctx.setLineDash([]);
          ctx.beginPath();
          ctx.moveTo(x - 4 * dpr, top + height);
          ctx.lineTo(x + 4 * dpr, top + height);
          ctx.lineTo(x, top + height - 6 * dpr);
          ctx.fill();
        }
      }

      // trigger: the level on the source's plot, the time on all
      if (trig.mode !== 'roll') {
        const col = cssVar('--trig');
        ctx.strokeStyle = col;
        ctx.fillStyle = col;
        ctx.lineWidth = dpr;
        ctx.setLineDash([6 * dpr, 4 * dpr]);
        if (sig === trig.source) {
          const y = u.valToPos(trig.level, 'y', true);
          if (y >= top && y <= top + height) {
            ctx.beginPath();
            ctx.moveTo(left, y);
            ctx.lineTo(left + width, y);
            ctx.stroke();
            // "T" tag on the right edge
            ctx.setLineDash([]);
            ctx.beginPath();
            ctx.moveTo(left + width, y);
            ctx.lineTo(left + width - 8 * dpr, y - 6 * dpr);
            ctx.lineTo(left + width - 8 * dpr, y + 6 * dpr);
            ctx.fill();
          }
        }
        if (trig.shown != null) {
          const x = xOf(trig.shown);
          if (inside(x)) {
            ctx.setLineDash([2 * dpr, 3 * dpr]);
            ctx.beginPath();
            ctx.moveTo(x, top);
            ctx.lineTo(x, top + height);
            ctx.stroke();
          }
        }
        ctx.setLineDash([]);
      }

      // markers
      for (const m of view.markers) {
        const col = markerColor(m);
        const active = m.id === view.active;
        ctx.strokeStyle = col;
        ctx.lineWidth = (active ? 1.75 : 1) * dpr;
        ctx.globalAlpha = active ? 1 : 0.7;
        if (m.t1 == null) ctx.setLineDash([5 * dpr, 4 * dpr]);
        const xs = m.t1 == null ? [xOf(m.t0)] : [xOf(m.t0), xOf(m.t1)];
        if (m.t1 != null) {
          const a = Math.max(left, xs[0]);
          const b = Math.min(left + width, xs[1]);
          if (b > a) {
            ctx.fillStyle = col + (active ? '22' : '10');
            ctx.fillRect(a, top, b - a, height);
          }
        }
        for (const x of xs) {
          if (!inside(x)) continue;
          ctx.beginPath();
          ctx.moveTo(x, top);
          ctx.lineTo(x, top + height);
          ctx.stroke();
        }
        ctx.setLineDash([]);
        ctx.globalAlpha = 1;
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
      const log = axes[sig].log;
      // only the range is shown: outline it, so it reads as data
      const edge = trace.range && !trace.mean ? { stroke: col + 'aa', width: 1, points: { show: false } } : hidden;
      const bands: uPlot.Band[] = [];
      if (trace.range) bands.push({ series: [2, 1], fill: col + (trace.mean ? '30' : '55') });
      if (trace.sigma) bands.push({ series: [4, 3], fill: col + '50' });
      const u = new uPlot(
        {
          width: el.clientWidth,
          height: el.clientHeight,
          scales: {
            x: { time: false, auto: false },
            y: { auto: true, range: yRange(sig), ...(log ? { distr: 3, log: 10 } : {}) },
          },
          legend: { show: false },
          cursor: { drag: { x: true, y: false, setScale: false }, points: { show: false }, sync: { key: SYNC_KEY } },
          series: [
            {},
            { ...edge },
            { ...edge },
            { ...hidden },
            { ...hidden },
            // zoomed in to single samples: a dot on each
            trace.mean
              ? { stroke: col, width: 1.5, points: { show: () => !!lod?.raw, size: 5, fill: col, stroke: col } }
              : { ...hidden },
          ],
          bands,
          axes: [
            {
              ...axis,
              splits: divisions(DIVS_X),
              size: last ? 28 : 6,
              // seconds from the trigger when there is one on screen, else from the right edge
              values: (_u, v) => (last ? v.map((x, k) => (k % 2 ? '' : fmtTime(x - origin(), 2))) : v.map(() => '')),
            },
            {
              ...axis,
              size: 78,
              splits: log ? logSplits : divisions(DIVS_Y),
              values: siValues(SIGNAL_INFO[sig].unit, log),
              stroke: col,
            },
          ],
          hooks: {
            draw: [overlay(sig)],
            setSelect: [
              (u) => {
                if (u.select.width > 3) {
                  const a = u.posToVal(u.select.left, 'x');
                  const b = u.posToVal(u.select.left + u.select.width, 'x');
                  view.hold(store);
                  view.addDual(a, b);
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
                const L = lod;
                readout = {
                  t: L.t[k] - origin(),
                  v: Object.fromEntries(
                    signals.map((s) => {
                      const c = L.ch[s];
                      return [s, c.avg[k] == null ? null : { avg: c.avg[k]!, min: c.min[k]!, max: c.max[k]! }];
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
      u.over.addEventListener('dblclick', onDbl(u));
      plots.push({ sig, u });
    });
    drawnKey = '';
  }

  /** time zero on the axis */
  function origin() {
    return trig.mode !== 'roll' && trig.shown != null ? trig.shown : span[1];
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

  function onDbl(u: uPlot) {
    return (e: MouseEvent) => {
      e.preventDefault();
      view.hold(store);
      view.addSingle(u.posToVal(e.offsetX, 'x'));
    };
  }

  function markersKey() {
    return view.markers.map((m) => `${m.id}:${m.t0}:${m.t1}`).join(',') + '/' + view.active;
  }

  let inflight = false;
  let markerToken = 0;

  function frame(now: number) {
    raf = requestAnimationFrame(frame);
    if (!plots.length || inflight || now - lastDraw < MIN_FRAME_MS) return;
    const key = [
      view.live ? store.version : `${view.end}`,
      view.divIndex,
      store.tStart,
      markersKey(),
      signals.map((s) => `${axes[s].auto},${axes[s].min},${axes[s].max},${axes[s].log}`).join(';'),
      `${trace.mean}${trace.range}${trace.sigma}`,
      `${trig.mode},${trig.source},${trig.level},${trig.shown}`,
      plots.map((p) => `${p.u.width}x${p.u.height}`).join(),
    ].join('|');
    if (key === drawnKey) return;
    drawnKey = key;
    lastDraw = now;
    const sp = view.span(store);
    const bins = Math.max(50, Math.round(plots[0].u.bbox.width / devicePixelRatio));
    inflight = true;
    store
      .lod(sp[0], sp[1], bins)
      .then((l) => {
        lod = l;
        span = sp;
        for (const { sig, u } of plots) {
          const c = l.ch[sig];
          // a log axis can't show zero or less
          const pos = (a: (number | null)[]) => (axes[sig].log ? a.map((x) => (x != null && x > 0 ? x : null)) : a);
          u.batch(() => {
            u.setData([l.t, pos(c.min), pos(c.max), pos(c.sdLo), pos(c.sdHi), pos(c.avg)] as uPlot.AlignedData, false);
            u.setScale('x', { min: sp[0], max: sp[1] });
          });
        }
        updateMarkers();
      })
      .catch((e) => console.warn('lod', e))
      .finally(() => (inflight = false));
  }

  function updateMarkers() {
    const u = plots[0]?.u;
    if (!u) return;
    const dpr = devicePixelRatio;
    const left = u.bbox.left / dpr;
    const width = u.bbox.width / dpr;
    if (left !== area.left || width !== area.width) area = { left, width };
    const x = (t: number) => left + u.valToPos(t, 'x');
    const tx = trig.shown == null ? null : x(trig.shown);
    trigX = tx != null && tx >= left && tx <= left + width ? tx : null;
    const g = [];
    const singles: Promise<{ m: Marker; x: number; v: PointValue }>[] = [];
    let dual: Promise<typeof dualStats> = Promise.resolve(null);
    for (const m of view.markers) {
      const x0 = x(m.t0);
      const x1 = m.t1 == null ? null : x(m.t1);
      const right = x1 ?? x0;
      if (right < left - 1 || x0 > left + width + 1) continue; // off screen
      g.push({ m, x0: Math.max(left, x0), x1: x1 == null ? null : Math.min(left + width, x1), c0: x0 < left, c1: x1 != null && x1 > left + width });
      if (m.t1 == null) singles.push(store.valueAt(m.t0).then((v) => ({ m, x: x0, v })));
      else if (m.id === view.active) {
        const xr = Math.min(left + width, x1!);
        dual = store.stats(m.t0, m.t1).then((s) => {
          if (!s.i.n) return null;
          const integral: Record<Signal, number> = { i: s.charge, v: NaN, p: s.energy };
          const vals = Object.fromEntries(signals.map((sig) => [sig, { mean: s[sig].mean, integral: integral[sig] }]));
          return { m, x: xr, vals: vals as Record<Signal, { mean: number; integral: number }> };
        });
      }
    }
    geo = g;
    // the newest answers win
    const token = ++markerToken;
    Promise.all([Promise.all(singles), dual]).then(([sv, d]) => {
      if (token !== markerToken) return;
      singleVals = sv;
      dualStats = d;
    });
  }

  // ---- dragging markers in the marker bar --------------------------------
  let drag: { id: number; edge: 0 | 1 | 'both'; offset: number } | null = null;

  function timeAtClient(clientX: number) {
    const r = root.getBoundingClientRect();
    return span[0] + ((clientX - r.left - area.left) / area.width) * (span[1] - span[0]);
  }

  function startDrag(e: PointerEvent, m: Marker, edge: 0 | 1 | 'both') {
    e.stopPropagation();
    view.active = m.id;
    view.hold(store);
    const t = timeAtClient(e.clientX);
    drag = { id: m.id, edge, offset: edge === 'both' ? m.t0 - t : 0 };
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }

  function moveDrag(e: PointerEvent) {
    if (!drag) return;
    drag.edge = view.moveMarker(drag.id, drag.edge, timeAtClient(e.clientX) + drag.offset);
  }

  function endDrag(e: PointerEvent) {
    drag = null;
    (e.currentTarget as HTMLElement).releasePointerCapture?.(e.pointerId);
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

  // rebuild when the plots' structure changes (after the host divs exist)
  $effect(() => {
    const key = signals.join() + signals.map((s) => axes[s].log).join() + trace.mean + trace.range + trace.sigma;
    void key;
    queueMicrotask(build);
  });

  function fmtVal(x: number, unit: string) {
    const p = prefixFor(x);
    return `${(x / p[0]).toFixed(3)} ${p[1]}${unit}`;
  }
</script>

<div class="scope" bind:this={root}>
  <div class="markers">
    {#each geo as g (g.m.id)}
      {@const col = markerColor(g.m)}
      {@const active = g.m.id === view.active}
      {#if g.x1 != null}
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="bracket"
          class:active
          class:c0={g.c0}
          class:c1={g.c1}
          style="left: {g.x0}px; width: {Math.max(2, g.x1 - g.x0)}px; --m: {col}"
          onpointerdown={(e) => startDrag(e, g.m, 'both')}
          onpointermove={moveDrag}
          onpointerup={endDrag}
        >
          {#if !g.c0}
            <span class="handle l" onpointerdown={(e) => startDrag(e, g.m, 0)} onpointermove={moveDrag} onpointerup={endDrag}></span>
          {/if}
          {#if !g.c1}
            <span class="handle r" onpointerdown={(e) => startDrag(e, g.m, 1)} onpointermove={moveDrag} onpointerup={endDrag}></span>
          {/if}
          <span class="tag">
            {markerName(g.m)} Δt {fmtTime(g.m.t1! - g.m.t0)}
            {#if active}<button class="x" onpointerdown={(e) => e.stopPropagation()} onclick={() => view.remove(g.m.id)} title="Delete (Del)">✕</button>{/if}
          </span>
        </div>
      {:else}
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="flag"
          class:active
          style="left: {g.x0}px; --m: {col}"
          onpointerdown={(e) => startDrag(e, g.m, 'both')}
          onpointermove={moveDrag}
          onpointerup={endDrag}
        >
          <span class="tag">
            {markerName(g.m)}
            {#if active}<button class="x" onpointerdown={(e) => e.stopPropagation()} onclick={() => view.remove(g.m.id)} title="Delete (Del)">✕</button>{/if}
          </span>
        </div>
      {/if}
    {/each}
    {#if trig.mode !== 'roll' && trigX != null}
      <span class="tflag" style="left: {trigX}px" title="Trigger">T</span>
    {/if}
    {#if !geo.length && trigX == null}
      <span class="hintbar" style="left: {area.left}px">drag across a plot: dual marker · double-click: single marker</span>
    {/if}
  </div>

  {#each signals as sig (sig)}
    {@const info = SIGNAL_INFO[sig]}
    <div class="row">
      <div class="host" bind:this={hosts[sig]}></div>
      <span class="name" style="color: var({info.color})">{info.name}</span>
      {#if readout && readout.v[sig]}
        {@const r = readout.v[sig]!}
        <span class="val" style="color: var({info.color})">
          {fmtVal(r.avg, info.unit)}
          <small>{fmtVal(r.min, info.unit)} … {fmtVal(r.max, info.unit)}</small>
        </span>
      {/if}
      {#if dualStats && dualStats.vals[sig]}
        {@const d = dualStats.vals[sig]}
        {@const flip = dualStats.x > area.left + area.width * 0.7}
        <span
          class="mstat"
          style="left: {flip ? dualStats.x - 6 : dualStats.x + 6}px; --m: {markerColor(dualStats.m)}{flip ? '; transform: translateX(-100%)' : ''}"
        >
          <b>μ</b> {fmtSI(d.mean, info.unit, 4)}
          {#if info.integral}<br /><b>∫</b> {fmtSI(d.integral, info.integral, 4)}{/if}
        </span>
      {/if}
      {#each singleVals as s (s.m.id)}
        {#if s.v}
          <span class="mval" style="left: {s.x + 4}px; --m: {markerColor(s.m)}">{fmtSI(s.v[sig], info.unit, 4)}</span>
        {/if}
      {/each}
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
    color: var(--scope-text);
  }
  .markers {
    position: relative;
    height: 26px;
    flex: none;
    border-bottom: 1px solid var(--scope-border);
    font: 12px ui-monospace, monospace;
  }
  .bracket {
    position: absolute;
    top: 3px;
    bottom: 3px;
    border: 1.5px solid var(--m);
    border-top: 0;
    border-radius: 0 0 3px 3px;
    background: color-mix(in srgb, var(--m) 12%, transparent);
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: grab;
    touch-action: none;
    opacity: 0.75;
  }
  .bracket.active {
    opacity: 1;
    background: color-mix(in srgb, var(--m) 22%, transparent);
    z-index: 2;
  }
  .bracket.c0 {
    border-left-style: dashed;
  }
  .bracket.c1 {
    border-right-style: dashed;
  }
  .handle {
    position: absolute;
    top: -3px;
    bottom: -3px;
    width: 10px;
    cursor: ew-resize;
  }
  .handle.l {
    left: -6px;
  }
  .handle.r {
    right: -6px;
  }
  .flag {
    position: absolute;
    top: 3px;
    bottom: 0;
    width: 0;
    border-left: 1.5px dashed var(--m);
    cursor: grab;
    touch-action: none;
    opacity: 0.8;
  }
  .flag.active {
    opacity: 1;
    z-index: 2;
  }
  .flag .tag {
    position: absolute;
    left: 3px;
    top: 0;
  }
  .tag {
    background: #000;
    color: #fff;
    border: 1px solid var(--m);
    border-radius: 3px;
    padding: 0 5px;
    white-space: nowrap;
    font-weight: 600;
    font-size: 11px;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    pointer-events: auto;
  }
  .x {
    border: 0;
    background: transparent;
    color: var(--m);
    padding: 0 2px;
    font-size: 11px;
    line-height: 1;
    cursor: pointer;
  }
  .tflag {
    position: absolute;
    top: 2px;
    transform: translateX(-50%);
    background: var(--trig);
    color: #000;
    font-weight: 700;
    font-size: 11px;
    padding: 0 5px;
    border-radius: 3px;
    pointer-events: none;
  }
  .hintbar {
    position: absolute;
    top: 5px;
    font-size: 11px;
    opacity: 0.7;
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
  .val,
  .mstat,
  .mval {
    position: absolute;
    font: 600 11px ui-monospace, monospace;
    pointer-events: none;
    padding: 1px 6px;
    border-radius: 3px;
    background: #000c;
    white-space: nowrap;
  }
  .name {
    top: 4px;
    left: 84px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .val {
    top: 4px;
    right: 12px;
    font-size: 12px;
  }
  .val small {
    font-weight: 400;
    opacity: 0.75;
    margin-left: 6px;
  }
  .mstat {
    top: 26px;
    color: #fff;
    border: 1px solid var(--m);
    font-weight: 400;
    line-height: 1.35;
  }
  .mstat b {
    color: var(--m);
  }
  .mval {
    bottom: 6px;
    color: #fff;
    border: 1px solid var(--m);
    font-weight: 400;
  }
  .t {
    position: absolute;
    right: 12px;
    bottom: 32px;
    font: 12px ui-monospace, monospace;
    background: #000a;
    padding: 1px 6px;
    border-radius: 3px;
    pointer-events: none;
  }
  .empty {
    margin: auto;
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
