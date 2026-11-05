<script lang="ts" module>
  /** what the logger shows: following the whole session (or its last `last` s), or a fixed span */
  export interface Annotation {
    /** FPGA time, s */
    t: number;
    text: string;
    /** unix ms, for the tooltip */
    wall?: number;
    /** received, sent, or a trigger match (sets the marker colour) */
    kind?: 'rx' | 'tx' | 'trigger';
  }
  /** a triggered capture, drawn as its span: from `pre` before the trigger to its end (null: still going) */
  export interface CaptureMark {
    t: number;
    pre: number;
    end: number | null;
    label: string;
  }
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
  import { steadyLinear, steadyLog } from './yAuto';
  // The logger: the session log on a wall-clock axis, one plot per signal
  // (min–max band and mean per interval). Wheel: zoom, shift+wheel: pan,
  // drag: zoom to it, double-click: the whole session, click: open that
  // moment in the scope (when its samples are still in memory).
  import { onMount } from 'svelte';
  import uPlot from 'uplot';
  import { themeVar } from './themeVar';
  import { chip, chipWidth } from './chip';
  import { Glide, union } from './glide';
  import 'uplot/dist/uPlot.min.css';
  import type { Signal } from './nbp';
  import { SIGNAL_INFO, type AxisCfg, type TraceCfg } from './Scope.svelte';
  import type { Lod, SampleStore } from './store';
  import { fmtSI, logSplits, prefixFor } from './si';
  import { fmtTime } from './view.svelte';
  import { drag as dragAct, store as save } from './drag';
  import HoverTip, { type TipRow } from './HoverTip.svelte';
  import type { TriggerState } from './trigger.svelte';

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
    annotations = [],
    onback,
    onforward,
    canBack = false,
    canForward = false,
    trig,
    showTrig = false,
    rolling = null,
    preS = 0,
    captures = [],
    picking = false,
    onpick,
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
    /** serial-monitor messages, drawn as vertical markers */
    annotations?: Annotation[];
    /** back / forward through the views shown (the app keeps the history) */
    onback?: () => void;
    onforward?: () => void;
    canBack?: boolean;
    canForward?: boolean;
    /** the trigger: its level lines (dragged to set) and its events, shown when `showTrig` */
    trig?: TriggerState;
    showTrig?: boolean;
    /** a triggered capture waiting: the stretch it would start with, `preS` back from the newest */
    rolling?: 'watch' | 'armed' | null;
    preS?: number;
    /** triggered captures made, drawn as spans */
    captures?: CaptureMark[];
    /** pick mode: a click on a plot sets the trigger there (its signal, its value) */
    picking?: boolean;
    onpick?: (sig: Signal, value: number) => void;
  } = $props();

  /** new log data redraws no faster than this, nor than twice what a redraw costs; the user's moves at once */
  const MIN_FRAME_MS = 50;
  // the messages changed: redraw the overlay once, next frame (a chatty port sends hundreds a second)
  let annDirty = false;
  $effect(() => {
    void annotations.length;
    annDirty = true;
  });
  // the trigger's lines, events, the window and the captures: redraw the overlay when they change
  $effect(() => {
    void [showTrig, rolling, preS, captures.length, captures.at(-1)?.end, trig?.levels().join(), trig?.source, trig?.logVersion];
    annDirty = true;
  });
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

  // theme colours, kept per theme (themeVar.ts): read on every frame
  const cssVar = themeVar;

  /** the span lv describes, now */
  function wanted(): [number, number] {
    const s = store.session;
    if (!lv.follow) return [lv.t0, lv.t1];
    const end = s.t_end;
    const start = lv.last > 0 ? Math.max(s.t_start, end - lv.last) : s.t_start;
    return end > start ? [start, end] : [end - 1, end];
  }

  /** what each auto axis shows now (kept steady: yAuto.ts) */
  const yShown: Record<string, [number, number]> = {};
  // Autoset, Show all: fit at once
  $effect(() => {
    const refit = () => {
      for (const k in yShown) delete yShown[k];
    };
    addEventListener('nebulosa:yfit', refit);
    return () => removeEventListener('nebulosa:yfit', refit);
  });
  function yRange(sig: Signal) {
    return (_u: uPlot, lo: number | null, hi: number | null): uPlot.Range.MinMax => {
      const cfg = axes[sig];
      if (cfg.log) {
        if (!cfg.auto) return [Math.max(cfg.min, 1e-15), Math.max(cfg.max, 2e-15)];
        if (lo == null || hi == null || !(hi > 0)) return [1e-9, 1];
        return (yShown[sig + 'log'] = steadyLog(yShown[sig + 'log'] ?? null, Math.max(lo, hi * 1e-9, 1e-15), hi));
      }
      if (!cfg.auto) return [cfg.min, cfg.max];
      if (lo == null || hi == null || !isFinite(lo) || !isFinite(hi)) return [0, 1];
      return (yShown[sig] = steadyLinear(yShown[sig] ?? null, lo, hi));
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

  const annColor = () => cssVar('--accent') || '#4da3ff';
  /** vertical lines for serial messages in view; labels on the top plot */
  function drawAnnotations(u: uPlot, top: boolean) {
    if (!annotations.length) return;
    const ctx = u.ctx;
    const { left, top: ct, width, height } = u.bbox;
    ctx.save();
    ctx.beginPath();
    ctx.rect(left, ct, width, height);
    ctx.clip();
    const accent = annColor();
    const tx = cssVar('--muted') || '#8a90a6';
    const trig = cssVar('--danger') || '#ff5a5a';
    ctx.font = '10px ui-monospace, monospace';
    ctx.textBaseline = 'top';
    const minX = u.valToPos(u.scales.x.min!, 'x', true);
    const maxX = u.valToPos(u.scales.x.max!, 'x', true);
    let lastLabel = -1e9;
    for (const a of annotations) {
      const x = u.valToPos(a.t + wallOffset, 'x', true);
      if (x < minX || x > maxX) continue;
      const col = a.kind === 'trigger' ? trig : a.kind === 'tx' ? tx : accent;
      ctx.strokeStyle = col + 'cc';
      ctx.lineWidth = a.kind === 'trigger' ? 1.75 : 1;
      ctx.setLineDash(a.kind === 'tx' ? [2, 2] : [3, 3]);
      ctx.beginPath();
      ctx.moveTo(x, ct);
      ctx.lineTo(x, ct + height);
      ctx.stroke();
      ctx.setLineDash([]);
      if (top && x - lastLabel > 8 * devicePixelRatio) {
        const label = a.text.length > 28 ? a.text.slice(0, 27) + '…' : a.text;
        lastLabel = x + chip(ctx, x, ct + 2 * devicePixelRatio, label, col, devicePixelRatio);
      }
    }
    ctx.restore();
  }

  /** jump the view to a clock time (HH:MM[:SS]) or a duration into the session (1m30s, 90s, 2:05) */
  function jumpTo(text: string): boolean {
    const s = session;
    if (!s.has_data) return false;
    const v = text.trim();
    if (!v) return false;
    let target: number | null = null;
    // HH:MM:SS (three parts) is a wall clock time today
    const hms = v.match(/^(\d{1,2}):(\d{2}):(\d{2})$/);
    if (hms && Number(hms[1]) <= 23) {
      const d = new Date();
      d.setHours(Number(hms[1]), Number(hms[2]), Number(hms[3]), 0);
      const w = d.getTime() / 1000 - wallOffset;
      if (w >= s.t_start && w <= s.t_end) target = w;
    }
    if (target == null) {
      // a duration into the session: 1h, 1m30s, 90s, or M:SS
      let secs = NaN;
      const mmss = v.match(/^(\d+):(\d{2})$/);
      const dur = v.match(/^(?:(\d+)h)?(?:(\d+)m)?(?:(\d+(?:\.\d+)?)s?)?$/i);
      if (mmss) secs = Number(mmss[1]) * 60 + Number(mmss[2]);
      else if (dur && (dur[1] || dur[2] || dur[3])) secs = Number(dur[1] ?? 0) * 3600 + Number(dur[2] ?? 0) * 60 + Number(dur[3] ?? 0);
      if (isFinite(secs)) target = s.t_start + secs;
    }
    if (target == null || !isFinite(target)) return false;
    const cur = lv.follow ? (lv.last > 0 ? lv.last : s.t_end - s.t_start) : Math.max(1e-3, lv.t1 - lv.t0);
    const t = Math.max(s.t_start, Math.min(s.t_end, target));
    lv = { follow: false, last: lv.last, t0: t - cur / 2, t1: t + cur / 2 };
    return true;
  }

  // "go to": a small box under the binoculars button, open only while used
  let gotoOpen = $state(false);
  let gotoBad = $state(false);
  let gotoEl = $state<HTMLInputElement>();
  $effect(() => {
    if (gotoOpen) gotoEl?.focus();
  });
  function onGotoKey(e: KeyboardEvent) {
    e.stopPropagation();
    if (e.key === 'Escape') gotoOpen = false;
    else if (e.key === 'Enter') {
      if (jumpTo((e.currentTarget as HTMLInputElement).value)) gotoOpen = false;
      else gotoBad = true;
    } else gotoBad = false;
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
            trace.mean ? { stroke: col, width: 1, paths: steps, points: { show: false } } : { ...hidden },
          ],
          bands,
          axes: [
            { ...axis, size: last ? 40 : 6, ...(last ? {} : { values: (_u: uPlot, v: number[]) => v.map(() => '') }) },
            { ...axis, size: 78, ...(log ? { splits: logSplits } : {}), values: siValues(SIGNAL_INFO[sig].unit), stroke: col },
          ],
          hooks: {
            draw: [(u) => drawAnnotations(u, idx === 0), (u) => drawCapture(u, sig, idx === 0)],
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
      // before uPlot's own (selection) and the click: grab a level line, or pick
      u.over.addEventListener('mousedown', levelDown(sig, u), { capture: true });
      u.over.addEventListener('mousemove', levelHover(sig, u));
      u.over.addEventListener('mousedown', onDown(u));
      plots.push({ sig, u });
    });
    drawnKey = '';
  }

  function onWheel(u: uPlot) {
    return (e: WheelEvent) => {
      e.preventDefault();
      // from where the view is going, not where it was last drawn: quick wheel
      // events each add up instead of starting from the same place
      const [a, b] = wanted();
      const w = b - a;
      // by distance: a mouse notch (~100 px) is a step, a trackpad's small deltas a little each
      const k = e.deltaMode === 1 ? 16 : e.deltaMode === 2 ? 400 : 1;
      if (e.shiftKey || Math.abs(e.deltaX) > Math.abs(e.deltaY)) {
        const d = (((e.shiftKey ? e.deltaY || e.deltaX : e.deltaX) * k) / Math.max(1, u.over.clientWidth)) * w;
        lv = { follow: false, last: lv.last, t0: a + d, t1: b + d };
      } else {
        const at = u.posToVal(e.offsetX, 'x') - wallOffset;
        const f = Math.exp(Math.max(-300, Math.min(300, e.deltaY * k)) * 0.0034);
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

  /** the trigger's level lines (source plot), its events, the waiting window and the captures */
  function drawCapture(u: uPlot, sig: Signal, top: boolean) {
    const ctx = u.ctx;
    const { left, top: ct, width, height } = u.bbox;
    const dpr = devicePixelRatio;
    const xMin = u.scales.x.min!, xMax = u.scales.x.max!;
    const X = (t: number) => u.valToPos(t + wallOffset, 'x', true);
    ctx.save();
    ctx.beginPath();
    ctx.rect(left, ct, width, height);
    ctx.clip();
    ctx.font = `${10 * dpr}px ui-monospace, monospace`;
    ctx.textBaseline = 'top';
    ctx.textAlign = 'left';
    const label = (x: number, text: string, col: string) => {
      const w = chipWidth(ctx, text, dpr);
      chip(ctx, Math.max(left, Math.min(x, left + width - w)), ct + 2 * dpr, text, col, dpr);
    };
    const span = (a: number, b: number, col: string) => {
      const x0 = Math.max(left, X(a)), x1 = Math.min(left + width, X(b));
      if (x1 <= x0) return;
      ctx.fillStyle = col;
      ctx.fillRect(x0, ct, x1 - x0, height);
    };
    // captures made: their span, the trigger a solid line
    const acc = cssVar('--accent') || '#4da3ff';
    for (const c of captures) {
      const end = c.end ?? store.session.t_end;
      if (end + wallOffset < xMin || c.t - c.pre + wallOffset > xMax) continue;
      span(c.t - c.pre, end, acc + '22');
      const x = X(c.t);
      ctx.strokeStyle = acc;
      ctx.lineWidth = 1.5 * dpr;
      ctx.beginPath();
      ctx.moveTo(x, ct);
      ctx.lineTo(x, ct + height);
      ctx.stroke();
      if (top) label(Math.max(left, X(c.t - c.pre)), c.label, acc);
    }
    // waiting: what a capture would start with if it fired now
    if (rolling && preS > 0) {
      const end = store.session.t_end;
      const hold = cssVar('--hold') || '#e8b339';
      span(end - preS, end, hold + (rolling === 'armed' ? '30' : '18'));
      if (top && end - preS + wallOffset < xMax) label(Math.max(left, X(end - preS)), `${rolling === 'armed' ? 'armed' : 'watching'} · ${fmtTime(preS)} before`, hold);
    }
    if (showTrig && trig && sig === trig.source) {
      const col = cssVar('--trig') || '#ff8c42';
      ctx.strokeStyle = col;
      ctx.fillStyle = col;
      ctx.lineWidth = dpr;
      for (const v of trig.levels()) {
        const y = u.valToPos(v, 'y', true);
        if (!(y >= ct && y <= ct + height)) continue;
        ctx.setLineDash([6 * dpr, 4 * dpr]);
        ctx.beginPath();
        ctx.moveTo(left, y);
        ctx.lineTo(left + width, y);
        ctx.stroke();
        ctx.setLineDash([]);
        ctx.beginPath();
        ctx.moveTo(left + width, y);
        ctx.lineTo(left + width - 8 * dpr, y - 6 * dpr);
        ctx.lineTo(left + width - 8 * dpr, y + 6 * dpr);
        ctx.fill();
      }
      // every event in view: a tick at the top
      const ev = trig.events;
      const a = xMin - wallOffset, b = xMax - wallOffset;
      let lo = 0, hi = ev.length;
      while (lo < hi) {
        const mid = (lo + hi) >> 1;
        if (ev[mid].t < a) lo = mid + 1;
        else hi = mid;
      }
      ctx.globalAlpha = 0.85;
      for (let k = lo, n = 0; k < ev.length && ev[k].t <= b && n < 4000; k++, n++) {
        const x = X(ev[k].t);
        ctx.beginPath();
        ctx.moveTo(x - 4 * dpr, ct);
        ctx.lineTo(x + 4 * dpr, ct);
        ctx.lineTo(x, ct + 7 * dpr);
        ctx.fill();
      }
      ctx.globalAlpha = 1;
    }
    ctx.restore();
  }

  // ---- the trigger level, dragged on its plot; pick mode ---------------------------
  const GRAB_PX = 6;
  function levelAt(sig: Signal, u: uPlot, y: number): number {
    if (!showTrig || !trig || sig !== trig.source) return -1;
    let best = -1, bestD = GRAB_PX + 1;
    trig.levels().forEach((v, k) => {
      const d = Math.abs(u.valToPos(v, 'y') - y);
      if (d < bestD) [best, bestD] = [k, d];
    });
    return best;
  }
  function levelHover(sig: Signal, u: uPlot) {
    return (e: MouseEvent) => {
      if (e.buttons) return;
      u.over.style.cursor = picking ? 'crosshair' : levelAt(sig, u, e.offsetY) >= 0 ? 'ns-resize' : '';
    };
  }
  function levelDown(sig: Signal, u: uPlot) {
    return (e: MouseEvent) => {
      if (e.button !== 0 || !trig) return;
      if (picking) {
        e.stopImmediatePropagation();
        e.preventDefault();
        const v = u.posToVal(e.offsetY, 'y');
        if (!axes[sig].log || v > 0) onpick?.(sig, v);
        return;
      }
      const k = levelAt(sig, u, e.offsetY);
      if (k < 0) return;
      e.stopImmediatePropagation();
      e.preventDefault();
      const t = trig;
      const topPx = u.over.getBoundingClientRect().top;
      const move = (ev: MouseEvent) => {
        let v = u.posToVal(Math.min(u.over.clientHeight, Math.max(0, ev.clientY - topPx)), 'y');
        if (axes[sig].log) v = Math.max(v, 1e-15);
        if (t.kind !== 'window') t.level = v;
        else if (k === 0) t.low = v;
        else t.high = v;
      };
      const up = () => {
        removeEventListener('mousemove', move);
        removeEventListener('mouseup', up);
      };
      addEventListener('mousemove', move);
      addEventListener('mouseup', up);
    };
  }

  /** what a redraw costs (asking for the data to drawn), ms, smoothed */
  let drawCost = 10;
  /** the view part of the last key: a change there is the user, answered at once */
  let drawnView = '';
  let shownSpan = '';

  /** the span on screen, gliding to the wanted one (zoom and pan animate) */
  const glide = new Glide();
  /** what the plotted data was fetched for: its span and s per column */
  let fetched = { a: NaN, b: NaN, res: NaN };

  function frame(now: number) {
    raf = requestAnimationFrame(frame);
    if (!plots.length) return;
    const real = wanted();
    // following: the window never runs ahead of the data it has (no empty strip at the right)
    let target = real;
    if (lv.follow && lod && fetched.b < real[1]) {
      const d = real[1] - fetched.b;
      target = [real[0] - d, real[1] - d];
    }
    // following, the window scrolls with the log: that mustn't lag
    const moving = glide.step(now, target, lv.follow);
    const shown = glide.shown!;
    const viewKey = [
      lv.follow ? `follow ${lv.last}` : real.join(),
      signals.map((s) => `${axes[s].auto},${axes[s].min},${axes[s].max},${axes[s].log}`).join(';'),
      plots.map((p) => `${p.u.width}x${p.u.height}`).join(),
      wallOffset,
    ].join('|');
    const moved = viewKey !== drawnView;
    // on screen: where the glide is now, with the data there is (the right data follows)
    const spKey = shown.join();
    if (lod && spKey !== shownSpan) {
      shownSpan = spKey;
      span = shown;
      for (const { u } of plots) u.setScale('x', { min: shown[0] + wallOffset, max: shown[1] + wallOffset });
      annDirty = false;
    }
    if (annDirty) {
      annDirty = false;
      for (const p of plots) p.u.redraw(false, false);
    }
    if (inflight) return;
    const key = `${lv.follow ? store.session.version : ''}|${viewKey}`;
    const px = Math.max(50, Math.round(plots[0].u.bbox.width / devicePixelRatio));
    const tw = real[1] - real[0];
    // gliding: data for everything the glide crosses; not following: half a window
    // more each side, so a pan shows data at once (all as fine as the end view needs)
    let want: [number, number] = moving ? union(shown, real) : real;
    if (!lv.follow) want = [want[0] - tw / 2, want[1] + tw / 2];
    // still: fetch again only if the view left what's there, or needs it finer
    const refine = !moving && !lv.follow && (fetched.a > real[0] || fetched.b < real[1] || fetched.res > (tw / px) * 1.5);
    if (key === drawnKey && !refine) return;
    if (!moved && !refine && now - lastDraw < Math.max(MIN_FRAME_MS, drawCost * 2)) return;
    drawnKey = key;
    drawnView = viewKey;
    lastDraw = now;
    inflight = true;
    const t0 = performance.now();
    // zoomed in past the log interval on data the sample buffer still holds: draw from the buffer,
    // down to single samples (the padding kept inside the buffer); else from the session log
    const iv = store.session.interval;
    const fromBuf = tw / px < iv && real[0] >= store.tStart && real[1] <= store.tEnd;
    if (fromBuf) want = [Math.max(want[0], store.tStart), Math.min(want[1], store.tEnd)];
    // as fine as the end view needs, over all of it
    const bins = Math.max(1, Math.min(8192, Math.round((px * (want[1] - want[0])) / tw)));
    (fromBuf ? store.lod(want[0], want[1], bins) : store.sessionLod(want[0], want[1], bins))
      .then((l) => {
        fetched = { a: want[0], b: want[1], res: (want[1] - want[0]) / bins };
        lod = l;
        const shown = glide.shown!;
        span = shown;
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
            u.setScale('x', { min: shown[0] + wallOffset, max: shown[1] + wallOffset });
          });
        }
        shownSpan = shown.join();
        drawCost = drawCost * 0.8 + (performance.now() - t0) * 0.2;
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
    <!-- as the Scope's: LIVE follows the newest entries, HOLD stays put (Space either way) -->
    <button
      class="badge"
      class:live={lv.follow}
      onclick={() => (lv = lv.follow ? { follow: false, last: lv.last, t0: span[0], t1: span[1] } : { ...lv, follow: true })}
      title={lv.follow ? 'Following the newest entries: click (or Space) to stay here' : 'Staying here: click (or Space) to follow the newest entries'}
    >
      {lv.follow ? 'LIVE' : 'HOLD'}
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
    <span class="gotowrap">
      <button
        class="ghost small icon"
        class:on={gotoOpen}
        onclick={() => ((gotoOpen = !gotoOpen), (gotoBad = false))}
        disabled={!session.has_data}
        title="Go to a clock time (HH:MM:SS) or a time into the session (1m30s, 90s, 2:05, 1h)"
        aria-label="Go to a time"
        aria-expanded={gotoOpen}
      >
        <svg viewBox="0 0 24 24" width="15" height="15" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <circle cx="6.5" cy="15.5" r="3.5" />
          <circle cx="17.5" cy="15.5" r="3.5" />
          <path d="M3 15.5 5 6.5a1.5 1.5 0 0 1 3 0v5.5M21 15.5 19 6.5a1.5 1.5 0 0 0-3 0v5.5M10 12.5h4" />
        </svg>
      </button>
      {#if gotoOpen}
        <span class="gotopop">
          <input
            bind:this={gotoEl}
            class="goto mono"
            class:bad={gotoBad}
            placeholder="14:32:05, 1m30s…"
            aria-label="Time to go to"
            onkeydown={onGotoKey}
            onblur={() => (gotoOpen = false)}
          />
          <span class="gotohint">{gotoBad ? 'Not a time in this session' : 'Enter: go · Esc: close'}</span>
        </span>
      {/if}
    </span>
    {#if onback}
      <span class="navbtns">
        <button class="ghost small" onclick={onback} disabled={!canBack} title="Back to the view before (Alt ←, the mouse's back button)" aria-label="Back">◀</button>
        <button class="ghost small" onclick={onforward} disabled={!canForward} title="Forward again (Alt →)" aria-label="Forward">▶</button>
      </span>
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
  .navbtns {
    display: inline-flex;
    gap: 2px;
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
  .gotowrap {
    position: relative;
    display: inline-flex;
  }
  .icon {
    display: inline-flex;
    align-items: center;
    padding: 3px 6px;
  }
  .icon.on {
    color: var(--accent);
    border-color: var(--accent);
  }
  .gotopop {
    position: absolute;
    top: calc(100% + 4px);
    left: 0;
    z-index: 20;
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 6px;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: var(--r);
    box-shadow: 0 8px 24px #0008;
  }
  .gotohint {
    font-size: 11px;
    color: var(--muted);
    white-space: nowrap;
  }
  /* (beats the app's full-width inputs) */
  .bar .goto.bad {
    border-color: var(--danger, #e5484d);
  }
  .bar .goto {
    width: 170px;
    padding: 2px 6px;
    font-size: 12px;
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

  /* a narrow window: the one-line hints give their room to the plots (Help › Keys has them all) */
  @media (max-width: 1300px) {
    .hint {
      display: none;
    }
  }
</style>
