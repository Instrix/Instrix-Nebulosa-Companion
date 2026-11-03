<script lang="ts">
  // The trigger's event log as a table, newest first (the latest 1000 rows
  // shown; all of them go to CSV). A summary on top: count, rate, width.
  // Click a row: show that event on screen.
  import type { TrigEvent } from './nbp';
  import { fmtSI } from './si';
  import { EVENT_LABEL, type TriggerState } from './trigger.svelte';
  import { fmtTime } from './view.svelte';

  const SHOWN = 1000;

  let {
    trig,
    unit,
    integralUnit,
    origin,
    onjump,
    onclear,
    onexport,
  }: {
    trig: TriggerState;
    /** the source's unit (peak, mean) */
    unit: string;
    /** C for current, J for power, '' for voltage */
    integralUnit: string;
    /** time zero for the t column */
    origin: number;
    onjump: (e: TrigEvent) => void;
    onclear: () => void;
    onexport: () => void;
  } = $props();

  const rows = $derived.by(() => {
    void trig.logVersion;
    return trig.events.slice(-SHOWN).reverse();
  });

  const summary = $derived.by(() => {
    void trig.logVersion;
    const ev = trig.events;
    if (!ev.length) return null;
    const span = ev[ev.length - 1].t - ev[0].t;
    let n = 0, sum = 0, lo = Infinity, hi = -Infinity;
    for (const e of ev)
      if (e.width != null && isFinite(e.width)) {
        n++;
        sum += e.width;
        lo = Math.min(lo, e.width);
        hi = Math.max(hi, e.width);
      }
    return {
      count: ev.length,
      rate: ev.length > 1 && span > 0 ? (ev.length - 1) / span : NaN,
      width: n ? { avg: sum / n, min: lo, max: hi } : null,
    };
  });

  const f = (x: number | null, u: string) => (x == null || !isFinite(x) ? '' : fmtSI(x, u, 4));
</script>

<div class="wrap">
  <div class="bar">
    {#if summary}
      <span><b>{summary.count.toLocaleString()}</b> events</span>
      {#if isFinite(summary.rate)}<span>{summary.rate >= 1 ? `${summary.rate.toFixed(2)} /s` : `every ${fmtTime(1 / summary.rate)}`}</span>{/if}
      {#if summary.width}
        <span>width avg {fmtTime(summary.width.avg)} <small>({fmtTime(summary.width.min)} … {fmtTime(summary.width.max)})</small></span>
      {/if}
    {:else}
      <span class="muted">
        {trig.detecting ? 'No events yet.' : 'Not detecting: choose a trigger mode, or turn on "Log events" in Roll.'}
      </span>
    {/if}
    <span class="spacer"></span>
    <button class="ghost small" onclick={onexport} disabled={!summary}>CSV</button>
    <button class="ghost small" onclick={onclear} disabled={!summary}>Clear</button>
  </div>
  <div class="list">
    <table>
      <thead>
        <tr>
          <th class="l">#</th>
          <th>t</th>
          <th class="l">event</th>
          <th>width</th>
          <th>peak</th>
          <th>mean</th>
          {#if integralUnit}<th>∫</th>{/if}
        </tr>
      </thead>
      <tbody>
        {#each rows as e (e.seq)}
          <tr class:shown={trig.shown != null && Math.abs(trig.shown - e.t) < 1e-9} onclick={() => onjump(e)}>
            <td class="l muted">{e.seq + 1}</td>
            <td>{fmtTime(e.t - origin)}</td>
            <td class="l">{EVENT_LABEL[e.what]}</td>
            <td>{e.width == null || !isFinite(e.width) ? '' : fmtTime(e.width)}</td>
            <td>{f(e.peak, unit)}</td>
            <td>{f(e.mean, unit)}</td>
            {#if integralUnit}<td>{f(e.integral, integralUnit)}</td>{/if}
          </tr>
        {/each}
      </tbody>
    </table>
    {#if trig.events.length > SHOWN}
      <p class="more muted">The latest {SHOWN} of {trig.events.length.toLocaleString()} shown; CSV has them all.</p>
    {/if}
  </div>
</div>

<style>
  .wrap {
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--panel);
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 4px 10px;
    font-size: 12px;
    border-bottom: 1px solid var(--border);
    font-variant-numeric: tabular-nums;
  }
  .spacer {
    flex: 1;
  }
  .list {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
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
    padding: 3px 10px;
    border-bottom: 1px solid var(--border);
  }
  td {
    text-align: right;
    padding: 2px 10px;
    white-space: nowrap;
    border-bottom: 1px solid color-mix(in srgb, var(--border) 50%, transparent);
  }
  .l {
    text-align: left;
  }
  tbody tr {
    cursor: pointer;
  }
  tbody tr:hover {
    background: color-mix(in srgb, var(--accent) 8%, transparent);
  }
  tr.shown {
    background: color-mix(in srgb, var(--live) 18%, transparent);
  }
  .muted {
    color: var(--muted);
  }
  .more {
    font-size: 11px;
    text-align: center;
    margin: 4px;
  }
  button.small {
    padding: 1px 8px;
    font-size: 12px;
  }
</style>
