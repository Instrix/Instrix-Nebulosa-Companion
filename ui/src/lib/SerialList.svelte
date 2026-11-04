<script lang="ts">
  // The serial monitor's messages as a list: newest first, a filter, click a row
  // to show that moment. Used as the Scope's "Serial" tab and in the Logger.
  import type { Annotation } from './LoggerView.svelte';
  import { fmtTime } from './view.svelte';

  let {
    messages,
    origin = null,
    wallClock = false,
    onjump,
    onclear,
    onsave,
  }: {
    messages: Annotation[];
    /** FPGA time the "t" column is relative to (the Scope); null: wall-clock times */
    origin?: number | null;
    /** show the time of day instead of a relative time */
    wallClock?: boolean;
    onjump: (t: number) => void;
    onclear?: () => void;
    onsave?: () => void;
  } = $props();

  let filter = $state('');
  const MAX = 500;

  const shown = $derived.by(() => {
    const f = filter.trim().toLowerCase();
    const out: Annotation[] = [];
    for (let k = messages.length - 1; k >= 0 && out.length < MAX; k--) {
      const m = messages[k];
      if (!f || m.text.toLowerCase().includes(f)) out.push(m);
    }
    return out;
  });
  const hits = $derived(messages.filter((m) => m.kind === 'trigger').length);

  function when(m: Annotation) {
    if (wallClock || origin == null) return new Date(m.wall ?? 0).toLocaleTimeString();
    return fmtTime(m.t - origin);
  }
</script>

<div class="slist">
  <div class="bar">
    <input class="filter" placeholder="Filter…" bind:value={filter} />
    <span class="muted small count">{messages.length.toLocaleString()} msg{hits ? ` · ${hits} trigger` : ''}{filter ? ` · ${shown.length} shown` : ''}</span>
    <span class="sp"></span>
    {#if onsave}<button class="ghost small" onclick={onsave} disabled={!messages.length}>Save…</button>{/if}
    {#if onclear}<button class="ghost small" onclick={onclear} disabled={!messages.length}>Clear</button>{/if}
  </div>
  <div class="rows">
    {#each shown as m (m.wall + m.text)}
      <button class="row" class:trig={m.kind === 'trigger'} class:tx={m.kind === 'tx'} onclick={() => onjump(m.t)} title="{new Date(m.wall ?? 0).toLocaleTimeString()} — show this moment">
        <span class="t">{when(m)}</span>
        <span class="k">{m.kind === 'trigger' ? '●' : m.kind === 'tx' ? '»' : ''}</span>
        <span class="msg">{m.text}</span>
      </button>
    {:else}
      <p class="empty muted">
        {messages.length ? 'Nothing matches the filter.' : 'No serial messages. Open a port in the Serial monitor settings; each line it receives shows here and as a marker on the plots.'}
      </p>
    {/each}
  </div>
</div>

<style>
  .slist {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    border: 1px solid var(--border);
    border-radius: var(--r);
    background: var(--panel);
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 8px;
    border-bottom: 1px solid var(--border);
  }
  .filter {
    flex: 0 1 200px;
    min-width: 80px;
    max-width: 220px;
    width: auto;
    padding: 2px 6px;
    font-size: 12px;
  }
  .sp {
    flex: 1;
  }
  .count {
    white-space: nowrap;
  }
  .rows {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    font: 12px ui-monospace, monospace;
  }
  .row {
    display: flex;
    gap: 10px;
    width: 100%;
    padding: 2px 8px;
    border: 0;
    border-radius: 0;
    background: none;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .row:hover {
    background: var(--bg);
  }
  .t {
    flex: none;
    width: 86px;
    color: var(--accent);
    text-align: right;
  }
  .k {
    flex: none;
    width: 10px;
    color: var(--danger);
  }
  .msg {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .row.trig .msg {
    color: var(--danger);
    font-weight: 600;
  }
  .row.tx .msg,
  .row.tx .k {
    color: var(--muted);
  }
  .empty {
    padding: 10px 12px;
    margin: 0;
    font-family: system-ui, sans-serif;
  }
</style>
