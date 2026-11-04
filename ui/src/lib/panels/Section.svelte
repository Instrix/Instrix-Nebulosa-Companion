<script lang="ts">
  // A sidebar section: a header that folds it, with a one-line summary while
  // folded. Open or folded is remembered per section.
  import type { Snippet } from 'svelte';

  let {
    id,
    title,
    summary = '',
    open: initial = true,
    tour,
    actions,
    children,
  }: {
    /** remembers open / folded under this name */
    id: string;
    title: string;
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
  function toggle() {
    open = !open;
    try {
      localStorage.setItem(key(), open ? '1' : '0');
    } catch {
      /* storage unavailable */
    }
  }
</script>

<section class="sec" class:open data-tour={tour}>
  <div class="head">
    <button class="toggle" onclick={toggle} aria-expanded={open} title={open ? 'Fold' : 'Unfold'}>
      <span class="chev" aria-hidden="true">›</span>
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
