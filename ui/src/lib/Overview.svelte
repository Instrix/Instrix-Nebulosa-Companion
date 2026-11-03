<script lang="ts">
  // The whole history at a glance, with a box for the scope window.
  // Drag the box to scroll, click elsewhere to jump there, wheel = time/div.
  // Dragging the box to the right edge goes back to LIVE. On HOLD the time
  // axis freezes too, so the box stays put while data keeps coming in; the
  // right tag counts what arrived since.
  import { onMount } from 'svelte';
  import type { Lod, SampleStore } from './store';
  import { fmtTime, markerColor, type View } from './view.svelte';

  let { store, view }: { store: SampleStore; view: View } = $props();

  const DATA_FRAME_MS = 120; // new data alone redraws at this pace; interaction redraws at once

  let canvas: HTMLCanvasElement;
  let wrap: HTMLDivElement;
  let raf = 0;
  let lastKey = '';
  let lastData = 0;
  let drag: { offset: number } | null = null;
  /** the time axis on screen: [t0, t1] */
  let axis: [number, number] = [0, 1];
  /** the axis frozen on HOLD */
  let frozen: [number, number] | null = null;
  let newer = $state('');

  function cssVar(name: string) {
    return getComputedStyle(wrap ?? document.documentElement).getPropertyValue(name).trim();
  }

  function axisSpan(): [number, number] {
    if (view.live) frozen = null;
    else if (!frozen) {
      const len = Math.max(store.tEnd - store.tStart, view.windowS);
      frozen = [store.tEnd - len, store.tEnd];
    }
    if (frozen) {
      // the window can be wider than the frozen axis after zooming out
      const [a, b] = frozen;
      return b - a >= view.windowS ? frozen : [b - view.windowS, b];
    }
    const len = Math.max(store.tEnd - store.tStart, view.windowS);
    return [store.tEnd - len, store.tEnd];
  }

  let cached: { l: Lod; axis: [number, number] } | null = null;
  let lodVersion = -1;
  let lodAxis = '';
  let inflight = false;
  let dirty = true;

  function draw(now: number) {
    raf = requestAnimationFrame(draw);
    const dpr = devicePixelRatio;
    const W = Math.round(canvas.clientWidth * dpr);
    const H = Math.round(canvas.clientHeight * dpr);
    const bins = Math.max(50, Math.round(W / dpr));

    // fetch: when the data or the axis changed, at most every DATA_FRAME_MS
    const ax = axisSpan();
    const axKey = `${ax[0]},${ax[1]},${bins}`;
    if (!inflight && (store.version !== lodVersion || axKey !== lodAxis) && now - lastData > DATA_FRAME_MS) {
      inflight = true;
      lastData = now;
      const v = store.version;
      store
        .lod(ax[0], ax[1], bins)
        .then((l) => {
          cached = { l, axis: ax };
          lodVersion = v;
          lodAxis = axKey;
          dirty = true;
        })
        .catch((e) => console.warn('overview lod', e))
        .finally(() => (inflight = false));
    }

    // paint: on new data, or at once when the view / markers change
    const viewKey = [
      view.live, view.end, view.divIndex, view.active,
      view.markers.map((m) => `${m.t0}:${m.t1}`).join(),
      W, H, store.empty,
    ].join('|');
    if (viewKey !== lastKey) dirty = true;
    if (!dirty) return;
    dirty = false;
    lastKey = viewKey;

    if (canvas.width !== W || canvas.height !== H) {
      canvas.width = W;
      canvas.height = H;
    }
    const ctx = canvas.getContext('2d')!;
    ctx.clearRect(0, 0, W, H);
    if (store.empty || !cached) return;

    const L = cached.l;
    axis = cached.axis;
    const [t0, t1] = axis;
    const label = fmtTime(-(t1 - Math.max(t0, store.tStart)));
    if (label !== historyLabel) historyLabel = label;
    const ahead = store.tEnd - t1;
    const nw = !view.live && ahead > 0.05 ? `+${fmtTime(ahead, 2)} ▸` : '';
    if (nw !== newer) newer = nw;
    const x = (t: number) => ((t - t0) / (t1 - t0)) * W;
    const T = L.t;
    const { min: IMIN, max: IMAX, avg: IAVG } = L.ch.i;
    const VAVG = L.ch.v.avg;

    const pad = 4 * dpr;
    const yScale = (lo: (number | null)[], hi: (number | null)[]) => {
      let a = Infinity, b = -Infinity;
      for (let k = 0; k < lo.length; k++) {
        if (lo[k] != null && lo[k]! < a) a = lo[k]!;
        if (hi[k] != null && hi[k]! > b) b = hi[k]!;
      }
      if (!(b > a)) (a -= 1), (b += 1);
      return (v: number) => H - pad - ((v - a) / (b - a)) * (H - 2 * pad);
    };
    const yi = yScale(IMIN, IMAX);
    const yv = yScale(VAVG, VAVG);

    // current: min..max bars, then the mean
    const I = cssVar('--trace-i');
    const V = cssVar('--trace-v');
    ctx.fillStyle = I + '55';
    for (let k = 0; k < T.length; k++) {
      if (IMIN[k] == null) continue;
      const top = yi(IMAX[k]!);
      ctx.fillRect(x(T[k]), top, Math.max(dpr, W / bins), Math.max(dpr, yi(IMIN[k]!) - top));
    }
    const line = (ys: (number | null)[], y: (v: number) => number, col: string) => {
      ctx.strokeStyle = col;
      ctx.lineWidth = dpr;
      ctx.beginPath();
      let pen = false;
      for (let k = 0; k < T.length; k++) {
        if (ys[k] == null) {
          pen = false;
          continue;
        }
        const px = x(T[k]);
        const py = y(ys[k]!);
        if (pen) ctx.lineTo(px, py);
        else ctx.moveTo(px, py);
        pen = true;
      }
      ctx.stroke();
    };
    line(VAVG, yv, V + 'aa');
    line(IAVG, yi, I);

    // markers
    for (const m of view.markers) {
      const col = markerColor(m);
      ctx.globalAlpha = m.id === view.active ? 0.6 : 0.35;
      ctx.fillStyle = col;
      const a0 = x(m.t0);
      const w = m.t1 == null ? 2 * dpr : Math.max(2 * dpr, x(m.t1) - a0);
      ctx.fillRect(a0, 0, w, H);
      ctx.globalAlpha = 1;
    }

    // gaps: missing samples
    ctx.fillStyle = cssVar('--danger');
    for (const g of L.gaps) ctx.fillRect(x(g) - dpr, H - 6 * dpr, 2 * dpr, 6 * dpr);

    // window box; dim what's outside it
    const [w0, w1] = view.span(store);
    const a = x(w0);
    const b = x(w1);
    ctx.fillStyle = '#000000a0';
    ctx.fillRect(0, 0, a, H);
    ctx.fillRect(b, 0, W - b, H);
    ctx.fillStyle = '#ffffff12';
    ctx.fillRect(a, 0, b - a, H);
    ctx.strokeStyle = view.live ? cssVar('--live') : cssVar('--hold');
    ctx.lineWidth = 2 * dpr;
    ctx.strokeRect(a + dpr, dpr, Math.max(2 * dpr, b - a - 2 * dpr), H - 2 * dpr);
  }

  function timeAt(e: PointerEvent | WheelEvent) {
    const r = canvas.getBoundingClientRect();
    const [t0, t1] = axis;
    return t0 + ((e.clientX - r.left) / r.width) * (t1 - t0);
  }

  function onDown(e: PointerEvent) {
    if (store.empty) return;
    const t = timeAt(e);
    const [w0, w1] = view.span(store);
    if (t >= w0 && t <= w1) {
      drag = { offset: w1 - t };
    } else {
      // jump: center the window there
      drag = { offset: view.windowS / 2 };
      view.moveTo(store, t + drag.offset);
    }
    canvas.setPointerCapture(e.pointerId);
  }

  function onMove(e: PointerEvent) {
    if (!drag) return;
    const end = timeAt(e) + drag.offset;
    // the right edge of a frozen axis stands for "now"
    if (end >= axis[1] - (axis[1] - axis[0]) * 0.005) {
      view.live = true;
      drag = null; // the axis unfreezes under the pointer: end the drag
    }
    else view.moveTo(store, end);
  }

  function onUp(e: PointerEvent) {
    drag = null;
    canvas.releasePointerCapture(e.pointerId);
  }

  function onWheel(e: WheelEvent) {
    e.preventDefault();
    if (e.shiftKey) view.pan(store, Math.sign(e.deltaY) * 0.25);
    else view.zoom(store, Math.sign(e.deltaY), timeAt(e));
  }

  onMount(() => {
    raf = requestAnimationFrame(draw);
    return () => cancelAnimationFrame(raf);
  });

  let historyLabel = $state('');
</script>

<div class="overview" bind:this={wrap}>
  <canvas
    bind:this={canvas}
    onpointerdown={onDown}
    onpointermove={onMove}
    onpointerup={onUp}
    onpointercancel={onUp}
    onwheel={onWheel}
  ></canvas>
  <span class="tag left">{historyLabel}</span>
  {#if newer}
    <button class="tag right newer" onclick={() => (view.live = true)} title="Back to LIVE">{newer}</button>
  {:else}
    <span class="tag right">{view.live ? 'now' : ''}</span>
  {/if}
</div>

<style>
  .overview {
    position: relative;
    height: 56px;
    background: var(--scope-bg);
    border: 1px solid var(--scope-border);
    border-radius: 6px;
    overflow: hidden;
  }
  canvas {
    width: 100%;
    height: 100%;
    display: block;
    cursor: grab;
    touch-action: none;
  }
  canvas:active {
    cursor: grabbing;
  }
  .tag {
    position: absolute;
    bottom: 2px;
    font: 10px ui-monospace, monospace;
    color: var(--scope-text);
    pointer-events: none;
  }
  .left {
    left: 6px;
  }
  .right {
    right: 6px;
  }
  .newer {
    pointer-events: auto;
    padding: 0 6px;
    border: 1px solid var(--hold);
    border-radius: 3px;
    background: #000c;
    color: var(--hold);
    cursor: pointer;
  }
</style>
