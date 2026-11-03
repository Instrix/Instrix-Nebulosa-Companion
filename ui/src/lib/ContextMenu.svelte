<script lang="ts">
  // The right-click menu (menu.svelte.ts): kept on screen; closes on a click
  // elsewhere, Escape, scrolling or resizing. Arrow keys and Enter work.
  import { tick } from 'svelte';
  import { closeMenu, menu, type MenuEntry, type MenuItem } from './menu.svelte';

  let el = $state<HTMLDivElement>();
  let pos = $state({ x: 0, y: 0 });
  let focus = $state(-1);

  const isItem = (e: MenuEntry): e is MenuItem => 'label' in e;
  const usable = (e: MenuEntry) => isItem(e) && !e.disabled && !!e.run;

  $effect(() => {
    if (!menu.items) return;
    pos = { x: menu.x, y: menu.y };
    focus = -1;
    tick().then(() => {
      if (!el) return;
      const r = el.getBoundingClientRect();
      pos = {
        x: Math.max(4, Math.min(menu.x, innerWidth - r.width - 4)),
        y: Math.max(4, menu.y + r.height > innerHeight - 4 ? menu.y - r.height : menu.y),
      };
      el.focus();
    });
  });

  function pick(it: MenuItem) {
    if (it.disabled || !it.run) return;
    closeMenu();
    it.run();
  }

  function step(d: number) {
    const items = menu.items ?? [];
    for (let k = 1; k <= items.length; k++) {
      const i = (focus + d * k + items.length * 2) % items.length;
      if (usable(items[i])) {
        focus = i;
        return;
      }
    }
  }

  function onKey(e: KeyboardEvent) {
    if (!menu.items) return;
    e.stopPropagation();
    if (e.key === 'Escape') closeMenu();
    else if (e.key === 'ArrowDown') step(1);
    else if (e.key === 'ArrowUp') step(-1);
    else if (e.key === 'Enter' && focus >= 0) pick(menu.items[focus] as MenuItem);
    else return;
    e.preventDefault();
  }

  function outside(e: Event) {
    if (menu.items && el && !el.contains(e.target as Node)) closeMenu();
  }
</script>

<svelte:window
  onpointerdown={outside}
  onwheel={outside}
  onresize={closeMenu}
  onblur={closeMenu}
/>

{#if menu.items}
  <div
    class="cmenu"
    role="menu"
    tabindex="-1"
    bind:this={el}
    style="left: {pos.x}px; top: {pos.y}px"
    onkeydown={onKey}
    oncontextmenu={(e) => e.preventDefault()}
  >
    {#each menu.items as it, i (i)}
      {#if 'sep' in it}
        <hr />
      {:else if 'head' in it}
        <div class="head">{it.head}</div>
      {:else}
        <button
          role="menuitem"
          class:focus={focus === i}
          class:danger={it.danger}
          disabled={it.disabled || !it.run}
          onclick={() => pick(it)}
          onpointerenter={() => (focus = usable(it) ? i : -1)}
        >
          <span class="tick">{it.checked ? '✓' : ''}</span>
          <span class="label">{it.label}</span>
          {#if it.hint}<span class="hint">{it.hint}</span>{/if}
        </button>
      {/if}
    {/each}
  </div>
{/if}

<style>
  .cmenu {
    position: fixed;
    z-index: 100;
    min-width: 200px;
    max-width: 340px;
    max-height: calc(100vh - 8px);
    overflow-y: auto;
    padding: 4px;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow: 0 8px 24px #0006;
    font-size: 13px;
    outline: none;
  }
  button {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    padding: 5px 8px;
    border: 0;
    border-radius: 5px;
    background: transparent;
    color: inherit;
    text-align: left;
    font: inherit;
    cursor: default;
  }
  button.focus {
    background: var(--accent);
    color: #fff;
  }
  button:disabled {
    opacity: 0.45;
  }
  button.danger:not(.focus) {
    color: var(--danger);
  }
  .tick {
    width: 12px;
    flex: none;
  }
  .label {
    flex: 1;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .hint {
    color: var(--muted);
    font-size: 11px;
    font-family: ui-monospace, monospace;
  }
  button.focus .hint {
    color: #fffc;
  }
  .head {
    padding: 4px 8px 2px 26px;
    color: var(--muted);
    font-size: 11px;
    font-weight: 600;
  }
  hr {
    border: 0;
    border-top: 1px solid var(--border);
    margin: 4px 2px;
  }
</style>
