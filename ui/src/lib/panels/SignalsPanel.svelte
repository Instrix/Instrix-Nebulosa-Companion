<script lang="ts">
  // Which signals show, how they're drawn (mean, min–max, ±σ), each one's axis, the sample buffer's size.
  import type { Signal } from '../nbp';
  import { SIGNAL_INFO, type AxisCfg, type TraceCfg } from '../Scope.svelte';
  import { fmtRate } from '../si';
  import { siInput } from '../siInput';
  import { fmtTime } from '../view.svelte';

  let {
    available,
    signals,
    shown = $bindable(),
    trace = $bindable(),
    axes = $bindable(),
    bufferMB = $bindable(),
    bufferChoices,
    bufferS,
    rateHz,
    onbuffer,
  }: {
    available: Signal[];
    /** the ones shown */
    signals: Signal[];
    shown: Record<Signal, boolean>;
    trace: TraceCfg;
    axes: Record<Signal, AxisCfg>;
    bufferMB: number;
    bufferChoices: number[];
    /** what the buffer holds at the rate, s */
    bufferS: number;
    rateHz: number;
    onbuffer: () => void;
  } = $props();
</script>

<section>
  <h2>Signals</h2>
  <div class="chips">
    {#each available as s (s)}
      <label class="chip" style="--c: var({SIGNAL_INFO[s].color})">
        <input type="checkbox" bind:checked={shown[s]} />
        {SIGNAL_INFO[s].name}
      </label>
    {/each}
  </div>
  <div class="axis">
    <span title="Each pixel column covers many samples: the line is their mean, the bands their min–max range and ±1 standard deviation">Trace</span>
    <div class="seg">
      <button class:on={trace.mean} onclick={() => (trace.mean = !trace.mean)} title="Mean line">Mean</button>
      <button class:on={trace.range} onclick={() => (trace.range = !trace.range)} title="Min–max band">Min–max</button>
      <button class:on={trace.sigma} onclick={() => (trace.sigma = !trace.sigma)} title="Mean ± 1 standard deviation">±σ</button>
    </div>
  </div>
  {#each signals as s (s)}
    {@const a = axes[s]}
    {@const unit = SIGNAL_INFO[s].unit}
    <div class="axis">
      <span style="color: var({SIGNAL_INFO[s].color})">{SIGNAL_INFO[s].name} axis</span>
      <div class="seg">
        <button class:on={a.log} onclick={() => (a.log = !a.log)} title="Logarithmic scale">Log</button>
        <button class:on={a.auto} onclick={() => (a.auto = true)}>Auto</button>
        <button class:on={!a.auto} onclick={() => (a.auto = false)}>Fixed</button>
      </div>
    </div>
    {#if !a.auto}
      <div class="row">
        <label>Min <input use:siInput={{ get: () => a.min, set: (v) => (a.min = v), unit }} /></label>
        <label>Max <input use:siInput={{ get: () => a.max, set: (v) => (a.max = v), unit }} /></label>
      </div>
    {/if}
  {/each}
  <label>
    Sample buffer (RAM)
    <select bind:value={bufferMB} onchange={onbuffer}>
      {#each bufferChoices as mb (mb)}<option value={mb}>{mb >= 1024 ? `${mb / 1024} GB` : `${mb} MB`}</option>{/each}
    </select>
    <small class="muted">every sample: {fmtTime(bufferS)} at {fmtRate(rateHz || 2e6)}</small>
  </label>
</section>
