<script lang="ts" module>
  import type { Signal } from './nbp';

  export type Scope = 'window' | 'run' | 'selection';

  export interface ChanSummary {
    mean: number;
    min: number;
    max: number;
    pp: number;
    std: number;
    rms: number;
  }

  export interface Summary {
    ch: Record<Signal, ChanSummary>;
    n: number;
    duration: number;
  }

  export interface AccumView {
    /** C, J, s since the last clear */
    charge: number;
    energy: number;
    seconds: number;
  }

  /** a marker in the list: dual = mean and integral over its span; single = values at its time */
  export interface MarkerRow {
    id: number;
    name: string;
    color: string;
    /** s; null for a single marker */
    dt: number | null;
    ch: Record<Signal, { mean: number; integral: number } | null>;
  }

  type MetricKey = keyof ChanSummary | 'n' | 'duration';

  export const METRICS: { key: MetricKey; label: string; title: string }[] = [
    { key: 'std', label: 'σ', title: 'sample standard deviation' },
    { key: 'min', label: 'min', title: 'lowest sample' },
    { key: 'max', label: 'max', title: 'highest sample' },
    { key: 'pp', label: 'p-p', title: 'max − min' },
    { key: 'rms', label: 'rms', title: 'root mean square' },
    { key: 'n', label: 'samples', title: 'samples in the span' },
    { key: 'duration', label: 'span', title: 'time covered' },
  ];
</script>

<script lang="ts">
  // Multimeter: the mean of each signal large, the chosen statistics under
  // it, over the window / since start / the selection; charge and energy
  // accumulators. Hold freezes the display.
  import { SIGNAL_INFO } from './Scope.svelte';
  import { fmtSI } from './si';
  import { fmtTime } from './view.svelte';

  let {
    summary,
    accum,
    signals,
    scope = $bindable('window'),
    hold = $bindable(false),
    hasSelection,
    onclear,
    markers,
    active,
    onpick,
    ondelete,
  }: {
    summary: Summary | null;
    accum: AccumView | null;
    signals: Signal[];
    scope: Scope;
    hold: boolean;
    hasSelection: boolean;
    onclear: () => void;
    markers: MarkerRow[];
    active: number | null;
    onpick: (id: number) => void;
    ondelete: (id: number) => void;
  } = $props();

  const STORAGE_KEY = 'nebulosa.meter.metrics';
  const DEFAULT: MetricKey[] = ['std', 'min', 'max', 'pp'];

  function load(): MetricKey[] {
    try {
      const v = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? 'null');
      if (Array.isArray(v)) return v;
    } catch {
      /* storage unavailable */
    }
    return DEFAULT;
  }

  let enabled = $state<MetricKey[]>(load());
  let configuring = $state(false);

  $effect(() => {
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(enabled));
    } catch {
      /* storage unavailable */
    }
  });

  const perChannel = $derived(METRICS.filter((m) => enabled.includes(m.key) && m.key !== 'n' && m.key !== 'duration'));
  const global = $derived(METRICS.filter((m) => enabled.includes(m.key) && (m.key === 'n' || m.key === 'duration')));

  function toggle(key: MetricKey) {
    enabled = enabled.includes(key) ? enabled.filter((k) => k !== key) : [...enabled, key];
  }

  function fmtN(n: number) {
    return n >= 1e9 ? `${(n / 1e9).toFixed(2)} G` : n >= 1e6 ? `${(n / 1e6).toFixed(2)} M` : n >= 1e3 ? `${(n / 1e3).toFixed(1)} k` : String(n);
  }
</script>

<div class="meter">
  <div class="bar">
    <div class="seg" role="group" aria-label="Statistics span">
      <button class:on={scope === 'window'} onclick={() => (scope = 'window')} title="What the screen shows">Window</button>
      <button class:on={scope === 'run'} onclick={() => (scope = 'run')} title="Since the stream started">Run</button>
      <button
        class:on={scope === 'selection'}
        disabled={!hasSelection}
        title={hasSelection ? 'The selected span' : 'Drag across the plot to select a span'}
        onclick={() => (scope = 'selection')}>Sel</button
      >
    </div>
    <button class="small hold" class:on={hold} onclick={() => (hold = !hold)} title="Freeze the readings">Hold</button>
    <button class="ghost small" class:on={configuring} onclick={() => (configuring = !configuring)} title="Choose statistics">⚙</button>
  </div>

  {#if configuring}
    <div class="config">
      {#each METRICS as m (m.key)}
        <label title={m.title}><input type="checkbox" checked={enabled.includes(m.key)} onchange={() => toggle(m.key)} /> {m.label}</label>
      {/each}
    </div>
  {/if}

  {#each signals as sig (sig)}
    {@const info = SIGNAL_INFO[sig]}
    {@const c = summary?.ch[sig]}
    <div class="card" style="--c: var({info.color})">
      <div class="head"><span class="name">{info.name}</span><span class="muted">mean</span></div>
      <div class="big">{c ? fmtSI(c.mean, info.unit, 5) : '–'}</div>
      {#if perChannel.length}
        <dl>
          {#each perChannel as m (m.key)}
            <dt title={m.title}>{m.label}</dt>
            <dd>{c ? fmtSI(c[m.key as keyof ChanSummary], info.unit, 4) : '–'}</dd>
          {/each}
        </dl>
      {/if}
    </div>
  {/each}

  {#if global.length}
    <div class="global">
      {#each global as m (m.key)}
        <span><span class="muted">{m.label}</span> {summary ? (m.key === 'n' ? fmtN(summary.n) : fmtTime(summary.duration)) : '–'}</span>
      {/each}
    </div>
  {/if}

  {#if markers.length}
    <div class="card marks">
      <div class="head"><span class="name">Markers</span></div>
      {#each markers as m (m.id)}
        <div
          class="mark"
          class:on={m.id === active}
          style="--m: {m.color}"
          role="button"
          tabindex="0"
          onclick={() => onpick(m.id)}
          onkeydown={(e) => e.key === 'Enter' && onpick(m.id)}
        >
          <div class="mhead">
            <span class="mname">{m.name}</span>
            <span class="muted">{m.dt == null ? 'at marker' : `Δt ${fmtTime(m.dt)}`}</span>
            <button class="ghost x" onclick={(e) => { e.stopPropagation(); ondelete(m.id); }} title="Delete">✕</button>
          </div>
          <dl>
            {#each signals as sig (sig)}
              {@const info = SIGNAL_INFO[sig]}
              {@const c = m.ch[sig]}
              <dt style="color: var({info.color})">{m.dt == null ? info.sym : `${info.sym} avg`}</dt>
              <dd>{c ? fmtSI(c.mean, info.unit, 4) : '–'}</dd>
              {#if m.dt != null && info.integral}
                <dt style="color: var({info.color})">∫{info.sym}</dt>
                <dd>{c ? fmtSI(c.integral, info.integral, 4) : '–'}</dd>
              {/if}
            {/each}
          </dl>
        </div>
      {/each}
    </div>
  {/if}

  <div class="card accum">
    <div class="head">
      <span class="name">Accumulators</span>
      <button class="ghost small" onclick={onclear} title="Start counting from zero">Clear</button>
    </div>
    <dl>
      <dt>charge</dt>
      <dd>{accum ? fmtSI(accum.charge, 'C', 5) : '–'}</dd>
      <dt></dt>
      <dd class="muted">{accum ? fmtSI(accum.charge / 3600, 'Ah', 5) : ''}</dd>
      <dt>energy</dt>
      <dd>{accum ? fmtSI(accum.energy, 'J', 5) : '–'}</dd>
      <dt></dt>
      <dd class="muted">{accum ? fmtSI(accum.energy / 3600, 'Wh', 5) : ''}</dd>
      <dt>time</dt>
      <dd>{accum ? fmtTime(accum.seconds) : '–'}</dd>
    </dl>
  </div>
</div>

<style>
  .meter {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 12px;
    overflow-y: auto;
    font-variant-numeric: tabular-nums;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .seg {
    display: inline-flex;
    border: 1px solid var(--border);
    border-radius: 6px;
    overflow: hidden;
    flex: 1;
  }
  .seg button {
    flex: 1;
    border: 0;
    border-radius: 0;
    background: transparent;
    padding: 4px 6px;
    font-size: 12px;
  }
  .seg button.on {
    background: var(--accent);
    color: #fff;
  }
  .hold.on {
    background: var(--hold);
    border-color: var(--hold);
    color: #000;
  }
  .config {
    display: flex;
    flex-wrap: wrap;
    gap: 4px 12px;
    font-size: 12px;
  }
  .config label {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .card {
    border: 1px solid var(--border);
    border-left: 3px solid var(--c, var(--border));
    border-radius: 6px;
    padding: 8px 10px;
    background: var(--panel);
  }
  .head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .name {
    color: var(--c, var(--text));
    font-weight: 600;
  }
  .big {
    font: 600 24px ui-monospace, monospace;
    margin: 2px 0 4px;
    white-space: nowrap;
  }
  dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 1px 10px;
    margin: 0;
    font: 12px ui-monospace, monospace;
  }
  dt {
    color: var(--muted);
  }
  dd {
    margin: 0;
    text-align: right;
  }
  .global {
    display: flex;
    justify-content: space-between;
    font-size: 12px;
  }
  .accum {
    --c: var(--accent);
  }
  .marks {
    --c: var(--sel);
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .mark {
    border: 1px solid var(--border);
    border-left: 3px solid var(--m);
    border-radius: 4px;
    padding: 4px 8px;
    cursor: pointer;
  }
  .mark.on {
    border-color: var(--m);
    background: color-mix(in srgb, var(--m) 10%, transparent);
  }
  .mhead {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
  }
  .mname {
    color: var(--m);
    font-weight: 700;
    font-family: ui-monospace, monospace;
  }
  .x {
    margin-left: auto;
    padding: 0 4px;
    font-size: 11px;
  }
  .muted {
    color: var(--muted);
  }
  button.small {
    padding: 2px 8px;
    font-size: 12px;
  }
</style>
