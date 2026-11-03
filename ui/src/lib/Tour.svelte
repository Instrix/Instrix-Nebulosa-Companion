<script lang="ts" module>
  export interface TourStep {
    /** [data-tour="..."] to point at; none: a card in the middle */
    target?: string;
    title: string;
    body: string;
    /** shown only when this is true when the step comes up */
    when?: () => boolean;
  }

  const SEEN_KEY = 'nebulosa.tourSeen';

  export function tourSeen(): boolean {
    try {
      return localStorage.getItem(SEEN_KEY) === '1';
    } catch {
      return true;
    }
  }

  export function markTourSeen() {
    try {
      localStorage.setItem(SEEN_KEY, '1');
    } catch {
      /* storage unavailable */
    }
  }
</script>

<script lang="ts">
  // The walkthrough: a spotlight on one part of the window and a card that
  // says what it is for; Next / Back / Skip, ← → Enter Esc. Steps whose
  // part isn't there (not connected, a panel hidden) are passed over.
  import { onMount, tick } from 'svelte';

  let {
    steps,
    onclose,
    ondemo,
    connected,
  }: {
    steps: TourStep[];
    onclose: () => void;
    /** connect the demo device (offered on the first card when nothing is connected) */
    ondemo?: () => void;
    connected: boolean;
  } = $props();

  let k = $state(0);
  let rect = $state<DOMRect | null>(null);
  let card = $state<HTMLDivElement>();
  let cardPos = $state({ x: 0, y: 0 });
  let raf = 0;

  const PAD = 6;
  /** the spotlight's edges, px */
  const box = $derived(rect ? { L: Math.max(0, rect.left - PAD), T: Math.max(0, rect.top - PAD), R: rect.right + PAD, B: rect.bottom + PAD } : null);

  function el(s: TourStep): HTMLElement | null {
    if (!s.target) return null;
    const e = document.querySelector<HTMLElement>(`[data-tour="${s.target}"]`);
    if (!e) return null;
    const r = e.getBoundingClientRect();
    return r.width > 2 && r.height > 2 ? e : null;
  }

  /** a step can be shown: its condition holds and its target is on the page */
  function usable(s: TourStep) {
    return (!s.when || s.when()) && (!s.target || !!el(s));
  }

  async function go(to: number, dir: 1 | -1) {
    let i = to;
    while (i >= 0 && i < steps.length && !usable(steps[i])) i += dir;
    if (i < 0) return;
    if (i >= steps.length) return finish();
    k = i;
    el(steps[k])?.scrollIntoView({ block: 'nearest', inline: 'nearest' });
    await tick();
    place();
  }

  function finish() {
    markTourSeen();
    onclose();
  }

  /** the spotlight on the target; the card beside it where there's room */
  function place() {
    const e = el(steps[k]);
    rect = e ? e.getBoundingClientRect() : null;
    if (!card) return;
    const cw = card.offsetWidth, ch = card.offsetHeight;
    const W = innerWidth, H = innerHeight, gap = 14;
    if (!rect) {
      cardPos = { x: (W - cw) / 2, y: (H - ch) / 2 };
      return;
    }
    const r = rect;
    const clampX = (x: number) => Math.max(10, Math.min(W - cw - 10, x));
    const clampY = (y: number) => Math.max(10, Math.min(H - ch - 10, y));
    if (r.right + gap + cw < W) cardPos = { x: r.right + gap, y: clampY(r.top) };
    else if (r.left - gap - cw > 0) cardPos = { x: r.left - gap - cw, y: clampY(r.top) };
    else if (r.bottom + gap + ch < H) cardPos = { x: clampX(r.left), y: r.bottom + gap };
    else if (r.top - gap - ch > 0) cardPos = { x: clampX(r.left), y: r.top - gap - ch };
    else cardPos = { x: clampX(r.right - cw - 10), y: clampY(r.bottom - ch - 10) }; // inside a big target
  }

  function follow() {
    place();
    raf = requestAnimationFrame(follow);
  }

  onMount(() => {
    go(0, 1);
    raf = requestAnimationFrame(follow);
    return () => cancelAnimationFrame(raf);
  });

  function onKey(e: KeyboardEvent) {
    if (e.key === 'ArrowRight' || e.key === 'Enter') go(k + 1, 1);
    else if (e.key === 'ArrowLeft') go(k - 1, -1);
    else if (e.key === 'Escape') finish();
    else return;
    e.preventDefault();
    e.stopPropagation();
  }

  /** where this step is among the ones that can be shown now */
  const position = $derived.by(() => {
    void k;
    const shown = steps.filter((s) => !s.when || s.when());
    return { n: shown.indexOf(steps[k]) + 1, of: shown.length };
  });

  function useDemo() {
    ondemo?.();
    // the panels change: on to the next step once they're there
    setTimeout(() => go(k + 1, 1), 400);
  }
</script>

<svelte:window onkeydowncapture={onKey} />

<!-- blocks the app while the tour is on -->
<div class="block" role="presentation"></div>
{#if rect && box}
  {@const { L, T, R, B } = box}
  <!-- the dim around the spotlight: four panels -->
  <div class="shade" style="left: 0; top: 0; right: 0; height: {T}px"></div>
  <div class="shade" style="left: 0; top: {B}px; right: 0; bottom: 0"></div>
  <div class="shade" style="left: 0; top: {T}px; width: {L}px; height: {B - T}px"></div>
  <div class="shade" style="left: {R}px; top: {T}px; right: 0; height: {B - T}px"></div>
  <div class="spot" style="left: {L}px; top: {T}px; width: {R - L}px; height: {B - T}px"></div>
{:else}
  <div class="dim"></div>
{/if}

<div class="card" bind:this={card} style="left: {cardPos.x}px; top: {cardPos.y}px" role="dialog" aria-label="Walkthrough" aria-live="polite">
  <div class="count">{position.n} / {position.of}</div>
  <h3>{steps[k].title}</h3>
  <p>{@html steps[k].body}</p>
  {#if k === 0 && !connected && ondemo}
    <button class="demo" onclick={useDemo}>Use the demo device for the tour</button>
  {/if}
  <div class="nav">
    <button class="ghost small skip" onclick={finish}>{position.n === position.of ? 'Close' : 'Skip the tour'}</button>
    {#if position.n > 1}<button class="ghost small" onclick={() => go(k - 1, -1)}>Back</button>{/if}
    <button class="primary small" onclick={() => go(k + 1, 1)}>{position.n === position.of ? 'Done' : 'Next'}</button>
  </div>
</div>

<style>
  .block {
    position: fixed;
    inset: 0;
    z-index: 80;
  }
  .dim {
    position: fixed;
    inset: 0;
    z-index: 81;
    background: #000a;
    pointer-events: none;
  }
  .shade {
    position: fixed;
    z-index: 81;
    background: #0009;
    pointer-events: none;
  }
  .spot {
    position: fixed;
    z-index: 81;
    border-radius: 8px;
    box-shadow: 0 0 0 3px var(--accent);
    pointer-events: none;
    transition:
      left 0.2s,
      top 0.2s,
      width 0.2s,
      height 0.2s;
  }
  .card {
    position: fixed;
    z-index: 82;
    width: min(340px, calc(100vw - 20px));
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 10px;
    padding: 14px 16px 12px;
    box-shadow: 0 10px 30px #0007;
    transition:
      left 0.2s,
      top 0.2s;
  }
  .count {
    font-size: 11px;
    color: var(--muted);
  }
  h3 {
    margin: 2px 0 6px;
    font-size: 15px;
  }
  p {
    margin: 0 0 10px;
    font-size: 13px;
    line-height: 1.45;
  }
  p :global(kbd) {
    font-family: ui-monospace, monospace;
    font-size: 11px;
    padding: 0 4px;
    border: 1px solid var(--border);
    border-radius: 3px;
  }
  .demo {
    width: 100%;
    margin-bottom: 10px;
  }
  .nav {
    display: flex;
    gap: 6px;
    justify-content: flex-end;
  }
  .skip {
    margin-right: auto;
  }
</style>
