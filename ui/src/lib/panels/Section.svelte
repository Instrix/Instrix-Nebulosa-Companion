<script lang="ts">
  // A sidebar section: a header that folds it, with a one-line summary while
  // folded. Open or folded is remembered per section.
  import type { Snippet } from 'svelte';
  import Icon from './Icon.svelte';

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
  </div>
  <!-- folded, the contents stay (their state with them), only hidden -->
  <div class="body" hidden={!open}>
    {@render children()}
  </div>
</section>

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
</style>
