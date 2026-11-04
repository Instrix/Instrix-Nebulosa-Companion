<script lang="ts">
  // Codes to units: each current range's full scale, the voltage's scale and offset.
  import type { Calibration } from '../nbp';
  import { siInput } from '../siInput';
  import Section from './Section.svelte';

  let {
    cal = $bindable(),
    ranges = 6,
    note,
    onapply,
    onreset,
  }: {
    cal: Calibration;
    /** current ranges shown */
    ranges?: number;
    note: string;
    onapply: () => void;
    onreset: () => void;
  } = $props();
  const calNote = $derived(note);
</script>

<Section id="calibration" icon="calibration" title="Calibration" open={false}>
    <div class="cal">
      <p class="muted small">Firmware scaling by default. I = code × full scale / 16384; V = code × scale + offset.</p>
      {#each Array.from({ length: ranges }, (_, r) => r) as r (r)}
        <label class="inline">
          Range {r} full scale
          <input use:siInput={{ get: () => cal.i_fs[r], set: (v) => (cal.i_fs[r] = v), unit: 'A' }} />
        </label>
      {/each}
      <label class="inline">
        Voltage scale (per code)
        <input use:siInput={{ get: () => cal.v_scale, set: (v) => (cal.v_scale = v), unit: 'V' }} />
      </label>
      <label class="inline">
        Voltage offset
        <input use:siInput={{ get: () => cal.v_offset, set: (v) => (cal.v_offset = v), unit: 'V' }} />
      </label>
      <div class="row">
        <button class="primary" onclick={onapply}>Apply</button>
        <button class="ghost" onclick={onreset}>Firmware defaults</button>
      </div>
      {#if calNote}<p class="muted small">{calNote}</p>{/if}
    </div>
</Section>
