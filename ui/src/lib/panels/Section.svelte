<script lang="ts">
  // A sidebar section: a header that folds it, with a one-line summary while
  // folded. Open or folded is remembered per section.
  import type { Snippet } from 'svelte';
  import Icon from './Icon.svelte';
  import { SECTION_HELP } from './sectionHelp';

  let {
    id,
    title,
    icon = '',
    summary = '',
    open: initial = true,
    tour,
    actions,
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
      if ((e as CustomEvent<string>).detail === id) set(true);
    };
    const all = (e: Event) => set(!!(e as CustomEvent<boolean>).detail);
    addEventListener('nebulosa:open-section', openOne);
    addEventListener('nebulosa:all-sections', all);
    return () => {
      removeEventListener('nebulosa:open-section', openOne);
      removeEventListener('nebulosa:all-sections', all);
    };
  });
  // the "?": a quick rundown of this section's options
  const help = $derived(SECTION_HELP[id]);
  let helpOpen = $state(false);
  /** shown over everything: moved to <body>, out of the sidebar's scrolling */
  function portal(node: HTMLElement) {
    document.body.appendChild(node);
    return { destroy: () => node.remove() };
  }
  function onHelpKey(e: KeyboardEvent) {
    if (helpOpen && e.key === 'Escape') {
      e.stopPropagation();
      e.preventDefault();
      helpOpen = false;
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

<section class="sec" class:open data-tour={tour} data-sec={id}>
  <div class="head">
    <button class="toggle" onclick={toggle} aria-expanded={open} title={open ? 'Fold' : 'Unfold'}>
      <span class="chev" aria-hidden="true">›</span>
      {#if icon}<Icon name={icon} />{/if}
      <h2>{title}</h2>
      {#if !open && summary}<span class="sum">{summary}</span>{/if}
    </button>
    {#if actions}<div class="actions">{@render actions()}</div>{/if}
    {#if help}
      <button class="help" onclick={() => (helpOpen = true)} title="{title}: what's here and how to use it" aria-label="Help: {title}">?</button>
    {/if}
  </div>
  <!-- folded, the contents stay (their state with them), only hidden -->
  <div class="body" hidden={!open}>
    {@render children()}
  </div>
</section>

<svelte:window onkeydowncapture={onHelpKey} />

{#if helpOpen && help}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div class="hbackdrop" role="presentation" use:portal onclick={() => (helpOpen = false)}>
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div class="hdialog" role="dialog" aria-label="Help: {title}" tabindex="-1" onclick={(e) => e.stopPropagation()}>
      <header>
        {#if icon}<Icon name={icon} />{/if}
        <h3>{title}</h3>
        <button class="ghost small hclose" onclick={() => (helpOpen = false)} aria-label="Close">✕</button>
      </header>
      <p class="intro">{help.intro}</p>
      <dl>
        {#each help.items as it (it.name)}
          <dt>{it.name}</dt>
          <dd>{it.text}</dd>
        {/each}
      </dl>
      {#if help.tips?.length}
        <h4>How to use</h4>
        <ul>
          {#each help.tips as t (t)}<li>{t}</li>{/each}
        </ul>
      {/if}
      <p class="more muted">More in Help (? or F1) and the user manual.</p>
    </div>
  </div>
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
  .hbackdrop {
    position: fixed;
    inset: 0;
    background: #0008;
    display: grid;
    place-items: center;
    z-index: 70;
  }
  .hdialog {
    width: min(540px, 94vw);
    max-height: 86vh;
    overflow: auto;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: var(--r-lg);
    padding: 14px 18px 16px;
    color: var(--text);
    font-size: 13px;
    line-height: 1.45;
  }
  .hdialog header {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .hdialog header :global(.icon) {
    color: var(--accent);
  }
  .hdialog h3 {
    margin: 0;
    font-size: 16px;
  }
  .hclose {
    margin-left: auto;
  }
  .intro {
    margin: 8px 0 10px;
  }
  .hdialog dl {
    display: grid;
    grid-template-columns: minmax(110px, max-content) 1fr;
    gap: 6px 14px;
    margin: 0;
  }
  .hdialog dt {
    font-weight: 600;
  }
  .hdialog dd {
    margin: 0;
    color: var(--muted);
  }
  .hdialog h4 {
    margin: 14px 0 4px;
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--muted);
  }
  .hdialog ul {
    margin: 0;
    padding-left: 18px;
  }
  .hdialog li {
    margin: 2px 0;
  }
  .more {
    margin: 12px 0 0;
    font-size: 12px;
  }
</style>
