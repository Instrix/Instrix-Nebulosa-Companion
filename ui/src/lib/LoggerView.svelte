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

  let {
    store,
    session,
    signals,
    axes,
    trace,
    wallOffset,
    lv = $bindable(),
    onopen,
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
  let readout = $state<{ t: number; v: Partial<Record<Signal, number | null>> } | null>(null);
  /** the span on screen, FPGA time */
  let span = $state<[number, number]>([0, 1]);

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
          cursor: { drag: { x: true, y: false, setScale: false }, points: { show: false }, sync: { key: 'nebulosa-logger' } },
          series: [{}, { ...hidden }, { ...hidden }, { ...hidden }, { ...hidden }, trace.mean ? { stroke: col, width: 1.25, points: { show: () => !!lod?.raw, size: 4, fill: col } } : { ...hidden }],
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
                const k = u.cursor.idx;
                if (k == null || u.cursor.left == null || u.cursor.left < 0 || !lod) {
                  readout = null;
                  return;
                }
                const L = lod;
                readout = { t: L.t[k], v: Object.fromEntries(signals.map((s) => [s, L.ch[s].avg[k]])) };
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
        const nw = Math.max(w * f, store.session.interval * 4);
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
    store
      .sessionLod(want[0], want[1], bins)
      .then((l) => {
        lod = l;
        span = want;
        const t = l.t.map((x) => x + wallOffset);
        for (const { sig, u } of plots) {
          const c = l.ch[sig];
          const pos = (a: (number | null)[]) => (axes[sig].log ? a.map((x) => (x != null && x > 0 ? x : null)) : a);
          u.batch(() => {
            u.setData([t, pos(c.min), pos(c.max), pos(c.sdLo), pos(c.sdHi), pos(c.avg)] as uPlot.AlignedData, false);
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
    <span class="mono muted">{fmtTime(span[1] - span[0])} shown · {fmtTime(session.interval)} intervals</span>
    {#if session.has_data}
      <span class="mono muted">since {fmtClock(session.t_start)}</span>
    {/if}
    <span class="hint muted">wheel: zoom · shift+wheel: pan · drag: zoom in · double-click: all · click: open in scope</span>
  </div>
  <div class="plots">
    {#each signals as sig (sig)}
      {@const info = SIGNAL_INFO[sig]}
      <div class="row">
        <div class="host" bind:this={hosts[sig]}></div>
        <span class="name" style="color: var({info.color})">{info.name}</span>
        {#if readout && readout.v[sig] != null}
          <span class="val" style="color: var({info.color})">{fmtSI(readout.v[sig]!, info.unit, 4)}</span>
        {/if}
      </div>
    {:else}
      <div class="empty">No signals shown.</div>
    {/each}
    {#if !session.has_data}
      <div class="empty overlay">The session log fills as data arrives: one entry per {fmtTime(session.interval)}.</div>
    {/if}
    {#if readout}<span class="t">{fmtClock(readout.t)}</span>{/if}
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
    flex: 1;
    min-height: 60px;
    border-bottom: 1px solid var(--scope-border);
  }
  .host {
    position: absolute;
    inset: 0;
  }
  .name,
  .val,
  .t {
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
  .val {
    top: 4px;
    right: 12px;
  }
  .t {
    right: 12px;
    bottom: 44px;
    font-weight: 400;
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
