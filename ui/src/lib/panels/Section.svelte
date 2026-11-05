<script lang="ts">
  // A sidebar section: a header that folds it, with a one-line summary while
  // folded. Open or folded is remembered per section.
  import { setContext, type Snippet } from 'svelte';
  import Icon from './Icon.svelte';
  import { SECTION_HELP } from './sectionHelp';
  import SectionHelpDialog from './SectionHelpDialog.svelte';

  let {
    id,
    title,
    icon = '',
    summary = '',
    open: initial = true,
    tour,
    actions,
    popout = false,
    children,
  }: {
    /** remembers open / folded under this name */
    id: string;
    title: string;
    /** a line-icon name (Icon.svelte) shown before the title */
    icon?: string;
    /** shown next to the title while folded */
    summary?: string;
    /** open the first time */
    open?: boolean;
    /** a walkthrough target */
    tour?: string;
    /** buttons at the header's right */
    actions?: Snippet;
    /** ⤢ pops it out into a floating window (moved, still live; a fixed size, its contents flow into columns) */
    popout?: boolean;
    children: Snippet;
  } = $props();

  const key = () => `nebulosa.section.${id}`;
  function load(): boolean {
    try {
      const v = localStorage.getItem(key());
      if (v === '1' || v === '0') return v === '1';
    } catch {
      /* storage unavailable */
    }
    return initial;
  }
  let open = $state(load());
  // the showcases open a section to show what's in it
  function set(want: boolean) {
    if (open === want) return;
    open = want;
    try {
      localStorage.setItem(key(), want ? '1' : '0');
    } catch {
      /* storage unavailable */
    }
  }
  $effect(() => {
    const openOne = (e: Event) => {
      if ((e as CustomEvent<string>).detail !== id) return;
      set(true);
      if (floating && winEl) {
        raise();
        winEl.classList.add('flash');
        setTimeout(() => winEl?.classList.remove('flash'), 700);
      }
    };
    const all = (e: Event) => set(!!(e as CustomEvent<boolean>).detail);
    // Ctrl+K: a section in its own window
    const pop = (e: Event) => {
      if ((e as CustomEvent<string>).detail === id && popout) (floating ? raise() : popOut());
    };
    const dockIt = (e: Event) => {
      if ((e as CustomEvent<string>).detail === id && floating) dock();
    };
    addEventListener('nebulosa:popout-section', pop);
    addEventListener('nebulosa:dock-section', dockIt);
    addEventListener('nebulosa:open-section', openOne);
    addEventListener('nebulosa:all-sections', all);
    return () => {
      removeEventListener('nebulosa:popout-section', pop);
      removeEventListener('nebulosa:dock-section', dockIt);
      removeEventListener('nebulosa:open-section', openOne);
      removeEventListener('nebulosa:all-sections', all);
    };
  });
  // the "?": a quick rundown of this section's options
  const help = $derived(SECTION_HELP[id]);
  let helpOpen = $state(false);
  let secEl = $state<HTMLElement>();

  // ---- popped out: a floating window -------------------------------------------
  // The same body (its controls, their state) moves into the window and back:
  // nothing is drawn twice. Where it was is remembered.
  // A fixed width (two columns of controls); its height follows its contents,
  // within the screen (taller contents scroll). No corner to drag.
  const WIN_W = 600;
  type Win = { x: number; y: number; w: number; h: number };
  const fkey = () => `nebulosa.section.${id}.float`;
  function loadWin(): { on: boolean; win: Win } {
    const d = { on: false, win: { x: 300, y: 72, w: WIN_W, h: 0 } };
    try {
      const v = JSON.parse(localStorage.getItem(fkey()) ?? 'null');
      if (v?.win && typeof v.win.x === 'number' && v.win.h > 0) return { on: !!v.on && popout, win: { x: v.win.x, y: v.win.y, w: WIN_W, h: v.win.h } };
    } catch {
      /* storage unavailable */
    }
    return d;
  }
  const saved = loadWin();
  let floating = $state(saved.on);
  let win = $state<Win>(saved.win);
  let bodyEl = $state<HTMLElement>();
  let winEl = $state<HTMLElement>();
  let winBody = $state<HTMLElement>();
  /** the newest window on top */
  let z = $state(0);
  // panels inside can lay out wider (and unfold their extras) when floating
  setContext('section-floating', () => floating);

  function saveWin() {
    try {
      localStorage.setItem(fkey(), JSON.stringify({ on: floating, win }));
    } catch {
      /* storage unavailable */
    }
  }
  function clamp() {
    const W = innerWidth, H = innerHeight;
    win.w = Math.min(WIN_W, W - 16);
    win.h = Math.min(Math.max(120, win.h), H - 16);
    win.x = Math.min(Math.max(8, win.x), W - win.w - 8);
    win.y = Math.min(Math.max(8, win.y), H - win.h - 8);
  }
  function popOut() {
    if (!win.h) win.h = 400;
    floating = true;
    set(true);
    clamp();
    raise();
    saveWin();
  }
  /** as tall as its contents (within the screen) */
  function fitHeight() {
    if (!winEl || !bodyEl) return;
    const head = (winEl.querySelector('.whead') as HTMLElement | null)?.offsetHeight ?? 36;
    // the contents' own height (the scroll area is at least as tall as the window), with its padding and border
    const h = Math.min(innerHeight - 100, head + bodyEl.offsetHeight + 28 + 2);
    if (Math.abs(h - win.h) < 2) return;
    win.h = h;
    clamp();
    saveWin();
  }
  // its height follows its contents (another mode shows more, say)
  $effect(() => {
    if (!floating || !bodyEl || !winEl) return;
    const ro = new ResizeObserver(() => requestAnimationFrame(fitHeight));
    ro.observe(bodyEl);
    return () => ro.disconnect();
  });
  function dock() {
    floating = false;
    saveWin();
    requestAnimationFrame(() => secEl?.scrollIntoView({ block: 'nearest' }));
  }
  function raise() {
    z = (globalThis as { __nebulosaWinZ?: number }).__nebulosaWinZ = ((globalThis as { __nebulosaWinZ?: number }).__nebulosaWinZ ?? 0) + 1;
  }
  // the window lives in the app's host (fixed, over everything, styled as the sidebar)
  function portal(node: HTMLElement) {
    (document.getElementById('fwin-host') ?? document.body).appendChild(node);
    return { destroy: () => node.remove() };
  }
  // the body goes where it's shown: the window, or back under the header
  $effect(() => {
    if (!bodyEl || !secEl) return;
    if (floating && winBody) winBody.appendChild(bodyEl);
    else if (bodyEl.parentElement !== secEl) secEl.appendChild(bodyEl);
  });
  $effect(() => {
    if (!floating) return;
    const on = () => (clamp(), fitHeight());
    addEventListener('resize', on);
    return () => removeEventListener('resize', on);
  });
  function dragWin(e: PointerEvent) {
    if (e.button !== 0 || (e.target as HTMLElement).closest('button')) return;
    e.preventDefault();
    const x0 = e.clientX - win.x, y0 = e.clientY - win.y;
    const move = (ev: PointerEvent) => {
      win.x = ev.clientX - x0;
      win.y = ev.clientY - y0;
      clamp();
    };
    const up = () => {
      removeEventListener('pointermove', move);
      removeEventListener('pointerup', up);
      saveWin();
    };
    addEventListener('pointermove', move);
    addEventListener('pointerup', up);
  }
  function winKey(e: KeyboardEvent) {
    // Esc docks it (from a field: the field keeps Esc)
    if (e.key === 'Escape' && !(e.target as HTMLElement).closest('input, select, textarea')) {
      e.stopPropagation();
      dock();
    }
  }

  function toggle() {
    open = !open;
    try {
      localStorage.setItem(key(), open ? '1' : '0');
    } catch {
      /* storage unavailable */
    }
  }
</script>

<section class="sec" class:open data-tour={tour} data-sec={id} bind:this={secEl}>
  <div class="head">
    <button class="toggle" onclick={toggle} aria-expanded={open} title={open ? 'Fold' : 'Unfold'}>
      <span class="chev" aria-hidden="true">›</span>
      {#if icon}<Icon name={icon} />{/if}
      <h2>{title}</h2>
      {#if !open && summary}<span class="sum">{summary}</span>{/if}
    </button>
    {#if actions}<div class="actions">{@render actions()}</div>{/if}
    {#if popout}
      <button class="pop" onclick={() => (floating ? dock() : popOut())} title={floating ? 'Back in the sidebar' : 'Open in its own window: bigger, movable, while you watch the plots'} aria-label={floating ? `Dock ${title}` : `Pop out ${title}`}>{floating ? '⤡' : '⤢'}</button>
    {/if}
    {#if help}
      <button class="help" onclick={() => (helpOpen = true)} title="{title}: what's here and how to use it" aria-label="Help: {title}">?</button>
    {/if}
  </div>
  {#if floating}
    <button class="placeholder" onclick={() => (raise(), winEl?.focus())} title="It's in its own window: click to bring it to the front">
      In its own window · <span class="dockit" role="presentation" onclick={(e) => (e.stopPropagation(), dock())}>⤡ dock here</span>
    </button>
  {/if}
  <!-- folded, the contents stay (their state with them), only hidden; popped out, they move to the window -->
  <div class="body" hidden={!open && !floating} bind:this={bodyEl}>
    {@render children()}
  </div>
</section>

{#if floating}
  <div
    class="fwin sec open"
    data-fid={id}
    data-tour="win-{id}"
    role="dialog"
    aria-label={title}
    tabindex="-1"
    use:portal
    bind:this={winEl}
    style="left: {win.x}px; top: {win.y}px; width: {win.w}px; height: {win.h}px; z-index: {z}"
    onpointerdown={raise}
    onkeydown={winKey}
  >
    <div class="whead" role="presentation" onpointerdown={dragWin}>
      {#if icon}<Icon name={icon} />{/if}
      <h2>{title}</h2>
      <span class="sp"></span>
      {#if help}
        <button class="help" onclick={() => (helpOpen = true)} title="{title}: what's here and how to use it" aria-label="Help: {title}">?</button>
      {/if}
      <button class="pop" onclick={dock} title="Back in the sidebar (Esc)" aria-label="Dock {title}">⤡</button>
    </div>
    <div class="wbody" bind:this={winBody}></div>
  </div>
{/if}

{#if helpOpen && help && secEl}
  <SectionHelpDialog sec={floating && winEl ? winEl : secEl} {id} {title} {icon} {help} onclose={() => (helpOpen = false)} />
{/if}

<style>
  .sec {
    gap: 0 !important;
  }
  .sec:global(.flash) {
    animation: sec-flash 0.6s ease-out;
    border-radius: var(--r-sm);
  }
  @keyframes sec-flash {
    from {
      background: color-mix(in srgb, var(--accent) 22%, transparent);
    }
    to {
      background: transparent;
    }
  }
  .head {
    display: flex;
    align-items: center;
    gap: 6px;
    min-height: 24px;
  }
  .toggle {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 2px 0;
    border: 0;
    background: transparent;
    color: inherit;
    text-align: left;
    cursor: pointer;
    border-radius: var(--r-sm);
  }
  .toggle:hover :global(h2) {
    color: var(--text);
  }
  .toggle :global(.icon) {
    color: var(--muted);
    transition: color 0.12s;
  }
  .open .toggle :global(.icon) {
    color: var(--accent);
  }
  .toggle:hover :global(.icon) {
    color: var(--text);
  }
  .chev {
    display: inline-block;
    width: 10px;
    color: var(--muted);
    font-size: 14px;
    line-height: 1;
    transition: transform 0.12s;
  }
  .open .chev {
    transform: rotate(90deg);
  }
  .sum {
    margin-left: auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12px;
    color: var(--muted);
  }
  .actions {
    display: flex;
    gap: 4px;
  }
  .body {
    display: flex;
    flex-direction: column;
    gap: 10px;
    margin-top: 10px;
  }
  .body[hidden] {
    display: none;
  }
  .pop {
    flex: none;
    width: 20px;
    height: 20px;
    padding: 0;
    border: 1px solid transparent;
    border-radius: var(--r-sm);
    background: transparent;
    color: var(--muted);
    font-size: 13px;
    line-height: 1;
    opacity: 0.6;
  }
  .sec:hover .pop,
  .fwin .pop,
  .pop:focus-visible {
    opacity: 1;
  }
  .pop:hover {
    color: var(--accent);
    border-color: var(--accent);
  }
  .placeholder {
    margin-top: 8px;
    padding: 8px;
    border: 1px dashed var(--border);
    border-radius: var(--r);
    background: transparent;
    color: var(--muted);
    font-size: 12px;
    text-align: center;
  }
  .dockit {
    color: var(--accent);
    cursor: pointer;
  }
  .dockit:hover {
    text-decoration: underline;
  }
  /* the floating window */
  .fwin {
    position: fixed;
    pointer-events: auto;
    display: flex !important;
    flex-direction: column;
    gap: 0 !important;
    padding: 0 !important;
    background: var(--panel);
    border: 1px solid var(--border) !important;
    border-radius: var(--r-lg);
    box-shadow: 0 18px 50px #0009;
    overflow: hidden;
    min-height: 120px;
  }
  .fwin:global(.flash) {
    animation: sec-flash 0.6s ease-out;
  }
  .whead {
    flex: none;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 10px 8px 14px;
    border-bottom: 1px solid var(--border);
    cursor: grab;
    user-select: none;
  }
  .whead:active {
    cursor: grabbing;
  }
  .whead :global(.icon) {
    color: var(--accent);
  }
  .sp {
    flex: 1;
  }
  .wbody {
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding: 12px 16px 16px;
  }
  /* wider: the contents flow into columns, each control kept whole */
  .wbody > :global(.body) {
    display: block;
    margin-top: 0;
    columns: 260px;
    column-gap: 24px;
    column-rule: 1px solid color-mix(in srgb, var(--border) 60%, transparent);
  }
  .wbody > :global(.body > *) {
    break-inside: avoid;
    margin-bottom: 10px;
  }
  /* a group of controls (the Logger's trigger) may run on into the next column, each of its controls whole */
  .wbody > :global(.body > .capfields) {
    break-inside: auto;
  }
  .wbody > :global(.body > .capfields > *) {
    break-inside: avoid;
  }
  /* one big editor (File output) takes the whole width; it lays itself out wide */
  .wbody > :global(.body > .ed) {
    column-span: all;
  }
  .wbody > :global(.body > :is(small, p)) {
    display: block;
  }
  .help {
    flex: none;
    width: 18px;
    height: 18px;
    padding: 0;
    border: 1px solid var(--border);
    border-radius: 50%;
    background: transparent;
    color: var(--muted);
    font-size: 11px;
    font-weight: 700;
    line-height: 1;
    cursor: help;
    opacity: 0.6;
    transition: opacity 0.12s, color 0.12s;
  }
  .sec:hover .help,
  .help:hover,
  .help:focus-visible {
    opacity: 1;
  }
  .help:hover {
    color: var(--accent);
    border-color: var(--accent);
  }
</style>
