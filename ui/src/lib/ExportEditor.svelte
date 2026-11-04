<script lang="ts" module>
  import type { ExportCfg, ExportField, ExportWhat } from './nbp';

  /** the logger's output: the app's own binary, or one of the export formats */
  export type OutKind = 'native' | 'csv' | 'txt' | 'jsonl' | 'hdf5';

  export interface OutputCfg {
    kind: OutKind;
    cfg: ExportCfg;
  }

  /** units a field can be written in: label, scale from SI */
  const UNITS: Partial<Record<ExportWhat, [string, number][]>> = {
    time: [['s', 1], ['ms', 1e3], ['µs', 1e6]],
    i: [['A', 1], ['mA', 1e3], ['µA', 1e6], ['nA', 1e9]],
    v: [['V', 1], ['mV', 1e3]],
    p: [['W', 1], ['mW', 1e3], ['µW', 1e6]],
    charge: [['C', 1], ['mC', 1e3], ['mAh', 1000 / 3600], ['µAh', 1e6 / 3600]],
    energy: [['J', 1], ['mJ', 1e3], ['mWh', 1000 / 3600], ['µWh', 1e6 / 3600]],
  };

  export const WHAT_LABEL: Record<ExportWhat, string> = {
    time: 'Time',
    i: 'Current',
    v: 'Voltage',
    p: 'Power',
    m: 'Math',
    count: 'Samples',
    range: 'Range',
    dirty: 'Dirty',
    charge: 'Charge (cumulative)',
    energy: 'Energy (cumulative)',
  };

  function unitOf(f: ExportField): string {
    const u = UNITS[f.what]?.find(([, s]) => Math.abs(s - f.scale) < s * 1e-9);
    return u ? u[0] : '';
  }

  export function field(what: ExportWhat, stat: ExportField['stat'] = 'mean', unit?: string, decimals?: number): ExportField {
    const units = UNITS[what];
    const u = units?.find(([n]) => n === unit) ?? units?.[0];
    const statTxt = stat === 'mean' ? '' : ` ${stat === 'std' ? 'σ' : stat}`;
    const label = `${WHAT_LABEL[what].replace(' (cumulative)', '')}${statTxt}${u ? ` (${u[0]})` : ''}`;
    return { what, stat, label, scale: u?.[1] ?? 1, decimals: decimals ?? (what === 'time' ? 6 : 6) };
  }

  export const PRESETS: { name: string; data: 'summary' | 'raw'; fields: () => ExportField[] }[] = [
    { name: 'Means: I, V, P', data: 'summary', fields: () => [field('time', 'mean', 's', 3), field('i', 'mean', 'mA', 4), field('v', 'mean', 'V', 4), field('p', 'mean', 'mW', 4)] },
    {
      name: 'Current: mean, min, max, σ',
      data: 'summary',
      fields: () => [field('time', 'mean', 's', 3), field('i', 'mean', 'mA', 4), field('i', 'min', 'mA', 4), field('i', 'max', 'mA', 4), field('i', 'std', 'mA', 4)],
    },
    {
      name: 'Battery: I, V, charge, energy',
      data: 'summary',
      fields: () => [field('time', 'mean', 's', 1), field('i', 'mean', 'mA', 4), field('v', 'mean', 'V', 4), field('charge', 'mean', 'mAh', 6), field('energy', 'mean', 'mWh', 6)],
    },
    { name: 'Every sample: I, V', data: 'raw', fields: () => [field('time', 'mean', 's', 7), field('i', 'mean', 'µA', 3), field('v', 'mean', 'V', 4)] },
  ];

  export function defaultOutput(): OutputCfg {
    return {
      kind: 'csv',
      cfg: {
        data: 'summary',
        format: { type: 'csv', separator: ',', decimal_comma: false, header: true },
        time: 'relative',
        fields: PRESETS[0].fields(),
      },
    };
  }

  export { unitOf, UNITS };
</script>

<script lang="ts">
  // The logger's output: format, data (summaries or every sample), and the
  // fields, with a live preview and the data rate it makes.
  import { getContext, onDestroy } from 'svelte';
  import type { Link } from './device';
  import { fmtTime } from './view.svelte';

  let {
    out = $bindable(),
    link,
    wallOffset,
    interval,
    rateHz,
    mathOn,
    disabled = false,
  }: {
    out: OutputCfg;
    link: Link | null;
    wallOffset: number;
    /** session-log interval, s */
    interval: number;
    /** samples per second */
    rateHz: number;
    mathOn: boolean;
    disabled?: boolean;
  } = $props();

  let preview = $state('');
  let customSep = $state('|');

  const SEPARATORS: [string, string][] = [
    [',', 'Comma ,'],
    [';', 'Semicolon ;'],
    ['\\t', 'Tab'],
    [' ', 'Space'],
  ];

  function setKind(k: OutKind) {
    out.kind = k;
    if (k === 'csv') out.cfg.format = { type: 'csv', separator: ',', decimal_comma: false, header: true };
    else if (k === 'txt') out.cfg.format = { type: 'txt', width: 14, header: true };
    else if (k === 'jsonl') out.cfg.format = { type: 'jsonl' };
    else if (k === 'hdf5') {
      out.cfg.format = { type: 'hdf5' };
      if (out.cfg.time === 'iso') out.cfg.time = 'unix';
    }
  }

  /** every sample: time needs 0.1 µs (samples are 0.5 µs apart at 2 MS/s); stats don't apply */
  function setRaw() {
    out.cfg.data = 'raw';
    for (const f of out.cfg.fields) {
      if (f.what === 'time' && out.cfg.time !== 'iso' && f.scale === 1) f.decimals = Math.max(f.decimals, 7);
      if (f.what === 'count') f.what = 'range';
    }
  }

  function applyPreset(i: number) {
    const p = PRESETS[i];
    if (!p) return;
    out.cfg.data = p.data;
    out.cfg.fields = p.fields();
  }

  function move(k: number, d: number) {
    const f = out.cfg.fields;
    const j = k + d;
    if (j < 0 || j >= f.length) return;
    [f[k], f[j]] = [f[j], f[k]];
  }

  /** what makes sense for the data: summaries have stats and counts, samples ranges and flags */
  const whats = $derived(
    (Object.keys(WHAT_LABEL) as ExportWhat[]).filter(
      (w) =>
        (w !== 'm' || mathOn) &&
        (out.cfg.data === 'summary' ? w !== 'range' && w !== 'dirty' : w !== 'count'),
    ),
  );

  function setWhat(f: ExportField, w: ExportWhat) {
    const n = field(w, f.stat);
    Object.assign(f, { what: w, scale: n.scale, label: n.label });
  }

  function setUnit(f: ExportField, u: string) {
    const s = UNITS[f.what]?.find(([n]) => n === u);
    if (!s) return;
    const old = unitOf(f);
    f.scale = s[1];
    if (old && f.label.includes(`(${old})`)) f.label = f.label.replace(`(${old})`, `(${u})`);
  }

  // live preview, from the latest data
  let timer: ReturnType<typeof setInterval> | undefined;
  async function refresh() {
    if (out.kind === 'native' || !link) {
      preview = '';
      return;
    }
    try {
      preview = await link.exportPreview($state.snapshot(out.cfg), wallOffset, 6);
    } catch (e) {
      preview = String(e);
    }
  }
  $effect(() => {
    void JSON.stringify(out);
    void link;
    refresh();
    clearInterval(timer);
    timer = setInterval(refresh, 1000);
  });
  onDestroy(() => clearInterval(timer));

  /** rows per second and bytes per row (from the preview), for the rate */
  const rate = $derived.by(() => {
    const rows = out.cfg.data === 'raw' ? rateHz || 2e6 : 1 / interval;
    let perRow: number;
    if (out.kind === 'hdf5') perRow = out.cfg.fields.length * 8;
    else if (out.kind === 'native') perRow = out.cfg.data === 'raw' ? 4 : 112;
    else {
      const lines = preview.split('\n').filter(Boolean);
      const body = lines.slice(out.cfg.format.type !== 'jsonl' && 'header' in out.cfg.format && out.cfg.format.header ? 1 : 0);
      perRow = body.length ? body.reduce((a, l) => a + l.length + 1, 0) / body.length : out.cfg.fields.length * 12;
    }
    return { rows, perSec: rows * perRow };
  });

  function fmtBytes(b: number) {
    return b >= 1e12 ? `${(b / 1e12).toFixed(1)} TB` : b >= 1e9 ? `${(b / 1e9).toFixed(1)} GB` : b >= 1e6 ? `${(b / 1e6).toFixed(1)} MB` : b >= 1e3 ? `${(b / 1e3).toFixed(0)} kB` : `${b.toFixed(0)} B`;
  }
  /** in a popped-out window (Section.svelte): room to lay the fields out wide */
  const floating = getContext<(() => boolean) | undefined>('section-floating') ?? (() => false);
</script>

<div class="ed" class:disabled class:wide={floating()}>
  <label>
    Format
    <select value={out.kind} onchange={(e) => setKind((e.currentTarget as HTMLSelectElement).value as OutKind)} {disabled}>
      <option value="native">Nebulosa (.nblog / .nbrec): smallest, reopens in the app</option>
      <option value="csv">CSV</option>
      <option value="txt">Text, fixed-width columns</option>
      <option value="jsonl">JSON Lines (one object per line)</option>
      <option value="hdf5">HDF5 (.h5: h5py, MATLAB)</option>
    </select>
  </label>

  <div class="seg wide">
    <button class:on={out.cfg.data === 'summary'} onclick={() => (out.cfg.data = 'summary')} {disabled} title="One row per interval: statistics of every sample in it">
      Per {fmtTime(interval)}
    </button>
    <button class:on={out.cfg.data === 'raw'} onclick={setRaw} {disabled} title="One row per sample, at the full rate">
      Every sample
    </button>
  </div>

  {#if out.kind !== 'native'}
    {#if out.cfg.format.type === 'csv'}
      {@const f = out.cfg.format}
      <div class="row">
        <label>
          Separator
          <select
            value={SEPARATORS.some(([s]) => s === f.separator) ? f.separator : 'custom'}
            onchange={(e) => {
              const v = (e.currentTarget as HTMLSelectElement).value;
              f.separator = v === 'custom' ? customSep : v;
            }}
            {disabled}
          >
            {#each SEPARATORS as [s, l] (s)}<option value={s}>{l}</option>{/each}
            <option value="custom">Custom…</option>
          </select>
        </label>
        {#if !SEPARATORS.some(([s]) => s === f.separator)}
          <label>Custom <input bind:value={f.separator} oninput={() => (customSep = f.separator)} maxlength="4" {disabled} /></label>
        {/if}
      </div>
      <label class="check"><input type="checkbox" bind:checked={f.decimal_comma} {disabled} /> Decimal comma (1,5)</label>
      <label class="check"><input type="checkbox" bind:checked={f.header} {disabled} /> Header row</label>
    {:else if out.cfg.format.type === 'txt'}
      {@const f = out.cfg.format}
      <div class="row">
        <label>Column width <input type="number" min="6" max="40" bind:value={f.width} {disabled} /></label>
        <label class="check"><input type="checkbox" bind:checked={f.header} {disabled} /> Header</label>
      </div>
    {/if}

    <label>
      Time
      <select bind:value={out.cfg.time} {disabled}>
        <option value="relative">Seconds from the start</option>
        <option value="unix">Unix time (s since 1970)</option>
        {#if out.kind !== 'hdf5'}<option value="iso">ISO 8601 (UTC)</option>{/if}
      </select>
    </label>

    <div class="fields-head">
      <span>Fields</span>
      <select onchange={(e) => { applyPreset(Number((e.currentTarget as HTMLSelectElement).value)); (e.currentTarget as HTMLSelectElement).value = ''; }} {disabled}>
        <option value="">Preset…</option>
        {#each PRESETS as p, i (p.name)}<option value={i}>{p.name}</option>{/each}
      </select>
    </div>
    <div class="fields">
      {#each out.cfg.fields as f, k (k)}
        <div class="field">
          <input class="lbl" bind:value={f.label} {disabled} title="Column name" />
          <div class="row">
            <select value={f.what} onchange={(e) => setWhat(f, (e.currentTarget as HTMLSelectElement).value as ExportWhat)} {disabled}>
              {#each whats as w (w)}<option value={w}>{WHAT_LABEL[w]}</option>{/each}
            </select>
            {#if out.cfg.data === 'summary' && ['i', 'v', 'p', 'm'].includes(f.what)}
              <select bind:value={f.stat} {disabled}>
                <option value="mean">mean</option><option value="min">min</option><option value="max">max</option><option value="std">σ</option>
              </select>
            {/if}
          </div>
          <div class="row">
            {#if UNITS[f.what]}
              <select value={unitOf(f)} onchange={(e) => setUnit(f, (e.currentTarget as HTMLSelectElement).value)} {disabled}>
                {#each UNITS[f.what]! as [u] (u)}<option value={u}>{u}</option>{/each}
              </select>
            {/if}
            {#if !['count', 'range', 'dirty'].includes(f.what) && !(f.what === 'time' && out.cfg.time === 'iso')}
              <label class="decl" title="Digits after the point">
                decimals <input class="dec" type="number" min="0" max="12" bind:value={f.decimals} {disabled} />
              </label>
            {/if}
          </div>
          <div class="ops">
            <button class="ghost small" onclick={() => move(k, -1)} disabled={disabled || k === 0} title="Up">↑</button>
            <button class="ghost small" onclick={() => move(k, 1)} disabled={disabled || k === out.cfg.fields.length - 1} title="Down">↓</button>
            <button class="ghost small" onclick={() => out.cfg.fields.splice(k, 1)} {disabled} title="Remove">✕</button>
          </div>
        </div>
      {/each}
      <button class="ghost small" onclick={() => out.cfg.fields.push(field('i'))} {disabled}>+ Field</button>
    </div>

    <div class="preview">
      <div class="ph"><span>Preview</span><span class="muted">latest rows</span></div>
      <pre>{preview || (link ? 'No data yet.' : 'Connect to see a preview.')}</pre>
    </div>
  {:else}
    <small class="muted">
      {out.cfg.data === 'raw'
        ? 'Every sample, 4 bytes each, plus the session log. Opens in the app; export it later to any format.'
        : 'The session log: min / max / mean / σ of every signal per interval. Opens in the app.'}
    </small>
  {/if}

  <small class="muted rate">
    ~{fmtBytes(rate.perSec)}/s · {fmtBytes(rate.perSec * 86400)} a day ({Math.round(rate.rows).toLocaleString()} rows/s)
  </small>
</div>

<style>
  .ed {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 13px;
  }
  label.check {
    flex-direction: row;
    align-items: center;
  }
  .row {
    display: flex;
    gap: 6px;
    align-items: center;
  }
  .row > * {
    flex: 1;
    min-width: 0;
  }
  .seg {
    display: inline-flex;
    border: 1px solid var(--border);
    border-radius: var(--r);
    overflow: hidden;
  }
  .seg.wide {
    display: flex;
  }
  .seg button {
    flex: 1;
    border: 0;
    border-radius: 0;
    background: transparent;
    padding: 4px 8px;
    font-size: 12px;
  }
  .seg button.on {
    background: var(--accent);
    color: #fff;
  }
  .fields-head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--muted);
  }
  .fields-head select {
    width: auto;
    font-size: 12px;
    padding: 2px 6px;
    text-transform: none;
  }
  .fields {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  /* in a popped-out window: the field cards side by side */
  .wide .fields {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(230px, 1fr));
  }
  .field {
    border: 1px solid var(--border);
    border-radius: var(--r);
    padding: 6px;
    display: flex;
    flex-direction: column;
    gap: 4px;
    position: relative;
  }
  .field .lbl {
    font-weight: 600;
    padding-right: 76px !important;
  }
  .field select,
  .field input {
    font-size: 12px;
    padding: 3px 5px !important;
  }
  .decl {
    flex-direction: row !important;
    align-items: center;
    gap: 4px !important;
    font-size: 11px !important;
    color: var(--muted);
  }
  .field .dec {
    width: 48px;
  }
  .ops {
    position: absolute;
    top: 7px;
    right: 8px;
    display: flex;
    gap: 2px;
  }
  .ops button {
    padding: 0 5px;
    font-size: 12px;
  }
  .preview {
    border: 1px solid var(--border);
    border-radius: var(--r);
    overflow: hidden;
  }
  .ph {
    display: flex;
    justify-content: space-between;
    font-size: 11px;
    padding: 3px 8px;
    border-bottom: 1px solid var(--border);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }
  pre {
    margin: 0;
    padding: 6px 8px;
    font: 11px/1.4 ui-monospace, monospace;
    overflow-x: auto;
    max-height: 140px;
    white-space: pre;
  }
  .muted {
    color: var(--muted);
  }
  .rate {
    font-variant-numeric: tabular-nums;
  }
  .disabled {
    opacity: 0.75;
  }
  button.small {
    padding: 1px 6px;
    font-size: 12px;
  }
</style>
