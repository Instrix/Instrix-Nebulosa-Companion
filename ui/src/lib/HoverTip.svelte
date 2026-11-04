<script module lang="ts">
  /** one signal's values under the cursor; min/max/sd only where the point stands for several samples */
  export interface TipRow {
    name: string;
    /** a CSS variable, e.g. --trace-i */
    color: string;
    unit: string;
    avg: number;
    min?: number;
    max?: number;
    sd?: number;
  }
</script>

<script lang="ts">
  // The values under the mouse, in a box next to it (kept inside the plots).
  import { fmtSI } from './si';

  let {
    x,
    y,
    boxW,
    boxH,
    title,
    note = '',
    rows,
  }: { x: number; y: number; boxW: number; boxH: number; title: string; note?: string; rows: TipRow[] } = $props();

  let w = $state(0);
  let h = $state(0);
  const GAP = 16;
  const left = $derived(x + GAP + w > boxW - 4 && x - GAP - w >= 4 ? x - GAP - w : x + GAP);
  const top = $derived(Math.max(4, Math.min(y + GAP, boxH - h - 4)));
  /** several samples behind the point: show their spread */
  const spread = $derived(rows.some((r) => r.min != null && r.max != null && r.max > r.min));
  const sd = $derived(spread && rows.some((r) => r.sd != null && r.sd > 0));
</script>

<div class="tip" bind:clientWidth={w} bind:clientHeight={h} style="left: {left}px; top: {top}px">
  <div class="head">
    <span>{title}</span>
    {#if note}<span class="note">{note}</span>{/if}
  </div>
  <table>
    {#if spread}
      <thead>
        <tr><th></th><th>mean</th><th>min</th><th>max</th>{#if sd}<th>σ</th>{/if}</tr>
      </thead>
    {/if}
    <tbody>
      {#each rows as r (r.name)}
        <tr style="color: var({r.color})">
          <th>{r.name}</th>
          <td class="main">{fmtSI(r.avg, r.unit, 4)}</td>
          {#if spread}
            <td>{r.min != null ? fmtSI(r.min, r.unit, 4) : ''}</td>
            <td>{r.max != null ? fmtSI(r.max, r.unit, 4) : ''}</td>
            {#if sd}<td>{r.sd != null ? fmtSI(r.sd, r.unit, 3) : ''}</td>{/if}
          {/if}
        </tr>
      {/each}
    </tbody>
  </table>
</div>

<style>
  .tip {
    position: absolute;
    z-index: 5;
    pointer-events: none;
    background: #0b0c12ee;
    color: #e9e9ed;
    border: 1px solid var(--scope-border);
    border-radius: var(--r);
    padding: 5px 8px 6px;
    font: 11px ui-monospace, monospace;
    box-shadow: 0 4px 14px #0008;
    white-space: nowrap;
  }
  .head {
    display: flex;
    gap: 12px;
    justify-content: space-between;
    margin-bottom: 3px;
    color: #c8cad4;
  }
  .note {
    opacity: 0.7;
  }
  table {
    border-collapse: collapse;
  }
  th,
  td {
    padding: 1px 0 1px 10px;
    text-align: right;
    font-weight: 400;
  }
  tbody th {
    text-align: left;
    padding-left: 0;
    font-weight: 600;
    text-transform: uppercase;
    font-size: 10px;
    letter-spacing: 0.04em;
  }
  thead th {
    color: #9398ab;
    font-size: 10px;
  }
  td {
    opacity: 0.8;
  }
  td.main {
    opacity: 1;
    font-weight: 600;
  }
</style>
