<script lang="ts">
  // The markers as a table under the plots: one row each, scrollable.
  // Click a row: make it active. ⤢: fit the screen to it. ✕: delete.
  import type { MarkerRow } from './Meter.svelte';
  import { SIGNAL_INFO } from './Scope.svelte';
  import { fmtSI } from './si';
  import { fmtTime } from './view.svelte';

  let {
    rows,
    math = false,
    active,
    origin,
    onpick,
    ondelete,
    onzoom,
    onclear,
    onmenu,
  }: {
    rows: (MarkerRow & { t0: number })[];
    /** a math channel column */
    math?: boolean;
    active: number | null;
    /** time zero for the t column (trigger, or the newest sample) */
    origin: number;
    onpick: (id: number) => void;
    ondelete: (id: number) => void;
    onzoom: (id: number) => void;
    onclear: () => void;
    /** right-click on a marker's row, or on the table (null) */
    onmenu?: (e: MouseEvent, id: number | null) => void;
  } = $props();

  const I = SIGNAL_INFO.i, V = SIGNAL_INFO.v, P = SIGNAL_INFO.p, M = SIGNAL_INFO.m;
  const f = (x: number | undefined, unit: string) => (x == null || !isFinite(x) ? '–' : fmtSI(x, unit, 4));
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="list" oncontextmenu={(e) => onmenu?.(e, null)}>
  <table>
    <thead>
      <tr>
        <th class="name">Marker</th>
        <th>t</th>
        <th>Δt</th>
        <th style="color: var({I.color})">I avg</th>
        <th style="color: var({I.color})">∫I</th>
        <th style="color: var({V.color})">V avg</th>
        <th style="color: var({P.color})">P avg</th>
        <th style="color: var({P.color})">∫P</th>
        {#if math}<th style="color: var({M.color})">{M.name} avg</th>{/if}
        <th class="act">
          {#if rows.length}<button class="ghost small" onclick={onclear} title="Remove all markers">Clear</button>{/if}
        </th>
      </tr>
    </thead>
    <tbody>
      {#each rows as m (m.id)}
        <tr class:on={m.id === active} style="--m: {m.color}" onclick={() => onpick(m.id)} oncontextmenu={(e) => onmenu?.(e, m.id)}>
          <td class="name"><span class="dot"></span>{m.name}</td>
          <td>{fmtTime(m.t0 - origin)}</td>
          <td>{m.dt == null ? '–' : fmtTime(m.dt)}</td>
          <td>{f(m.ch.i?.mean, I.unit)}</td>
          <td>{m.dt == null ? '' : f(m.ch.i?.integral, 'C')}</td>
          <td>{f(m.ch.v?.mean, V.unit)}</td>
          <td>{f(m.ch.p?.mean, P.unit)}</td>
          <td>{m.dt == null ? '' : f(m.ch.p?.integral, 'J')}</td>
          {#if math}<td>{f(m.ch.m?.mean, M.unit)}</td>{/if}
          <td class="act">
            <button class="ghost small" title="Fit the screen to this marker" onclick={(e) => { e.stopPropagation(); onzoom(m.id); }}>⤢</button>
            <button class="ghost small" title="Delete" onclick={(e) => { e.stopPropagation(); ondelete(m.id); }}>✕</button>
          </td>
        </tr>
      {:else}
        <tr class="empty">
          <td colspan={math ? 10 : 9}>No markers. Drag across a plot for a dual marker, double-click for a single one (or D / M).</td>
        </tr>
      {/each}
    </tbody>
  </table>
</div>

<style>
  .list {
    height: 100%;
    min-height: 0;
    overflow-y: auto;
    border: 1px solid var(--border);
    border-radius: var(--r);
    background: var(--panel);
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font: 12px ui-monospace, monospace;
    font-variant-numeric: tabular-nums;
  }
  thead th {
    position: sticky;
    top: 0;
    background: var(--panel);
    color: var(--muted);
    font-weight: 600;
    text-align: right;
    padding: 4px 10px;
    border-bottom: 1px solid var(--border);
    z-index: 1;
  }
  td {
    text-align: right;
    padding: 3px 10px;
    border-bottom: 1px solid color-mix(in srgb, var(--border) 50%, transparent);
    white-space: nowrap;
  }
  .name {
    text-align: left;
  }
  td.name {
    color: var(--m);
    font-weight: 700;
  }
  .dot {
    display: inline-block;
    width: 8px;
    height: 8px;
    border-radius: var(--r-sm);
    background: var(--m);
    margin-right: 6px;
  }
  tbody tr {
    cursor: pointer;
  }
  tbody tr:hover {
    background: color-mix(in srgb, var(--m, var(--accent)) 8%, transparent);
  }
  tr.on {
    background: color-mix(in srgb, var(--m) 16%, transparent);
  }
  .act {
    text-align: right;
    width: 1%;
  }
  .act button {
    padding: 0 6px;
    font-size: 12px;
  }
  .empty td {
    text-align: center;
    color: var(--muted);
    cursor: default;
    padding: 12px;
  }
</style>
