<script lang="ts" module>
  /** one thing the search can find: an action, a settings section, an option in one */
  export interface Command {
    /** what it shows */
    label: string;
    /** what kind: Action, Settings, View, Help… */
    group: string;
    /** where it is, or what it does, in a few words */
    detail?: string;
    /** its key, if it has one */
    hint?: string;
    /** more words it is found by */
    keywords?: string;
    disabled?: boolean;
    run: () => void;
  }
</script>

<script lang="ts">
  // Ctrl+K: find any setting or action by typing a few words of it, and go
  // there (switching view, opening the section) or do it. Also a quick way to
  // learn the keys: each action shows its own.

  let { commands, onclose }: { commands: Command[]; onclose: () => void } = $props();

  let q = $state('');
  let sel = $state(0);
  let input = $state<HTMLInputElement>();
  let listEl = $state<HTMLDivElement>();

  /** every word typed found in the label, group, detail or keywords; label matches first */
  function score(c: Command, words: string[]): number {
    const label = c.label.toLowerCase();
    const rest = `${c.group} ${c.detail ?? ''} ${c.keywords ?? ''}`.toLowerCase();
    let s = 0;
    for (const w of words) {
      const li = label.indexOf(w);
      if (li === 0) s += 4;
      else if (li > 0) s += label[li - 1] === ' ' ? 3 : 2;
      else if (rest.includes(w)) s += 1;
      else return -1;
    }
    return s;
  }

  const shown = $derived.by(() => {
    const words = q.toLowerCase().split(/\s+/).filter(Boolean);
    if (!words.length) return commands.filter((c) => c.group !== 'Option');
    return commands
      .map((c, i) => ({ c, s: score(c, words), i }))
      .filter((x) => x.s >= 0)
      .sort((a, b) => b.s - a.s || a.i - b.i)
      .slice(0, 60)
      .map((x) => x.c);
  });

  $effect(() => {
    void q;
    sel = 0;
  });
  $effect(() => {
    input?.focus();
  });

  function go(c: Command | undefined) {
    if (!c || c.disabled) return;
    onclose();
    // after the dialog has gone (the action may open another, or move focus)
    requestAnimationFrame(() => c.run());
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      onclose();
    } else if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      const n = shown.length;
      if (!n) return;
      sel = (sel + (e.key === 'ArrowDown' ? 1 : -1) + n) % n;
      listEl?.querySelector(`[data-i="${sel}"]`)?.scrollIntoView({ block: 'nearest' });
    } else if (e.key === 'Enter') {
      e.preventDefault();
      go(shown[sel]);
    }
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<div class="pback" role="presentation" onclick={onclose}>
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div class="palette" role="dialog" aria-label="Find a setting or action" tabindex="-1" onclick={(e) => e.stopPropagation()}>
    <input
      bind:this={input}
      bind:value={q}
      onkeydown={onKey}
      placeholder="Find a setting or action: trigger level, baud, log every sample, export…"
      aria-label="Search"
      spellcheck="false"
    />
    <div class="list" bind:this={listEl} role="listbox">
      {#each shown as c, i (c.group + c.label + (c.detail ?? ''))}
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <div
          class="item"
          class:on={i === sel}
          class:off={c.disabled}
          role="option"
          aria-selected={i === sel}
          tabindex="-1"
          data-i={i}
          onmousemove={() => (sel = i)}
          onclick={() => go(c)}
        >
          <span class="grp">{c.group}</span>
          <span class="lbl">{c.label}{#if c.detail}<span class="det">{' · '}{c.detail}</span>{/if}</span>
          {#if c.hint}<kbd>{c.hint}</kbd>{/if}
        </div>
      {:else}
        <p class="none">Nothing found. Try other words, or open Help (F1).</p>
      {/each}
    </div>
    <p class="foot">↑ ↓ choose · Enter go · Esc close</p>
  </div>
</div>

<style>
  .pback {
    position: fixed;
    inset: 0;
    background: #0007;
    z-index: 80;
    display: flex;
    justify-content: center;
    align-items: flex-start;
    padding-top: 12vh;
  }
  .palette {
    width: min(640px, 94vw);
    max-height: 70vh;
    display: flex;
    flex-direction: column;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: var(--r-lg);
    box-shadow: 0 20px 60px #0009;
    overflow: hidden;
  }
  input {
    margin: 10px;
    padding: 9px 12px;
    font-size: 15px;
  }
  .list {
    overflow-y: auto;
    padding: 0 6px 6px;
  }
  .item {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px 8px;
    border-radius: var(--r);
    cursor: pointer;
    font-size: 13px;
  }
  .item.on {
    background: color-mix(in srgb, var(--accent) 16%, var(--panel));
  }
  .item.off {
    opacity: 0.45;
    cursor: default;
  }
  .grp {
    flex: none;
    width: 74px;
    font-size: 11px;
    color: var(--muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .lbl {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .det {
    color: var(--muted);
  }
  kbd {
    flex: none;
    font: 11px ui-monospace, monospace;
    padding: 1px 6px;
    border: 1px solid var(--border);
    border-radius: 4px;
    color: var(--muted);
  }
  .none {
    padding: 10px 12px;
    color: var(--muted);
    margin: 0;
  }
  .foot {
    margin: 0;
    padding: 6px 12px;
    border-top: 1px solid var(--border);
    font-size: 11px;
    color: var(--muted);
  }
</style>
