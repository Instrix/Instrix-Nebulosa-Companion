<script lang="ts">
  // The trigger: display mode, source, type and its settings, noise / holdoff / filter, position.
  import type { Signal } from '../nbp';
  import { SIGNAL_INFO } from '../Scope.svelte';
  import Section from './Section.svelte';
  import TriggerFields from './TriggerFields.svelte';
  import { KIND_LABEL, type TrigMode, type TriggerState } from '../trigger.svelte';

  let {
    trig,
    available,
    onlevel50,
    onarm,
  }: {
    trig: TriggerState;
    /** the signals there are */
    available: Signal[];
    /** the level halfway between the screen's min and max */
    onlevel50: () => void;
    /** Single: arm again */
    onarm: () => void;
  } = $props();

  function setMode(m: TrigMode) {
    trig.mode = m;
  }

  // the help's snapshot shows every option, even those this mode hides
  let preview = $state(false);
  $effect(() => {
    const on = (e: Event) => (preview = (e as CustomEvent<string | null>).detail === 'trigger');
    addEventListener('nebulosa:help-preview', on);
    return () => removeEventListener('nebulosa:help-preview', on);
  });

  function level50() {
    onlevel50();
  }
</script>

<Section id="trigger" icon="trigger" title="Trigger" tour="trigger" popout summary={{ roll: 'Roll', auto: 'Auto', normal: 'Normal', single: 'Single' }[trig.mode] + (trig.detecting ? ` · ${SIGNAL_INFO[trig.source].sym} ${KIND_LABEL[trig.kind] ?? trig.kind}` : '')}>
  <div class="seg wide" role="group" aria-label="Display mode">
    {#each [['roll', 'Roll'], ['auto', 'Auto'], ['normal', 'Normal'], ['single', 'Single']] as [m, label] (m)}
      <button class:on={trig.mode === m} onclick={() => setMode(m as TrigMode)}>{label}</button>
    {/each}
  </div>
  {#if trig.mode === 'roll'}
    <label class="check" title="Detect and list events while free-running">
      <input type="checkbox" bind:checked={trig.log} /> Log events
    </label>
  {/if}
  {#if trig.detecting || preview}
    <TriggerFields {trig} {available} onlevel50={level50} />

    {#if trig.mode !== 'roll' || preview}
      <label>
        Position {Math.round(trig.position * 100)} % pre-trigger
        <input type="range" min="0.1" max="0.9" step="0.05" bind:value={trig.position} />
      </label>
    {/if}
    {#if trig.mode === 'single' || preview}
      <button class="primary" onclick={onarm}>Arm</button>
    {/if}
  {/if}
</Section>
