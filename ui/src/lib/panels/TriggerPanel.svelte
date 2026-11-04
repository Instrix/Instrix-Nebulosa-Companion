<script lang="ts">
  // The trigger: display mode, source, type and its settings, noise / holdoff / filter, position.
  import type { Signal } from '../nbp';
  import { SIGNAL_INFO } from '../Scope.svelte';
  import { siInput } from '../siInput';
  import Section from './Section.svelte';
  import { KIND_LABEL, type KindType, type TrigMode, type TriggerState } from '../trigger.svelte';

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

<Section id="trigger" icon="trigger" title="Trigger" tour="trigger" summary={{ roll: 'Roll', auto: 'Auto', normal: 'Normal', single: 'Single' }[trig.mode] + (trig.detecting ? ` · ${SIGNAL_INFO[trig.source].sym} ${KIND_LABEL[trig.kind] ?? trig.kind}` : '')}>
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
    {@const unit = SIGNAL_INFO[trig.source].unit}
    <div class="axis">
      <span>Source</span>
      <div class="seg">
        {#each available as sg (sg)}
          <button class:on={trig.source === sg} onclick={() => (trig.source = sg)} style="color: {trig.source === sg ? '' : `var(${SIGNAL_INFO[sg].color})`}">{SIGNAL_INFO[sg].sym}</button>
        {/each}
      </div>
    </div>
    <label>
      Type
      <select bind:value={trig.kind}>
        {#each Object.entries(KIND_LABEL) as [k, label] (k)}<option value={k as KindType}>{label}</option>{/each}
      </select>
    </label>

    {#if trig.kind === 'edge'}
      <div class="axis">
        <span>Edge</span>
        <div class="seg">
          <button class:on={trig.slope === 'rise'} onclick={() => (trig.slope = 'rise')}>↑ Rise</button>
          <button class:on={trig.slope === 'fall'} onclick={() => (trig.slope = 'fall')}>↓ Fall</button>
          <button class:on={trig.slope === 'both'} onclick={() => (trig.slope = 'both')} title="Either">↕</button>
        </div>
      </div>
    {:else if trig.kind === 'window'}
      <div class="axis">
        <span>When</span>
        <div class="seg">
          <button class:on={trig.when === 'enter'} onclick={() => (trig.when = 'enter')} title="Entering the band">Enter</button>
          <button class:on={trig.when === 'exit'} onclick={() => (trig.when = 'exit')} title="Leaving the band">Exit</button>
          <button class:on={trig.when === 'both'} onclick={() => (trig.when = 'both')}>Both</button>
        </div>
      </div>
    {:else}
      <div class="axis">
        <span>{trig.kind === 'pulse' ? 'Pulse' : 'Stays'}</span>
        <div class="seg">
          <button class:on={trig.polarity === 'high'} onclick={() => (trig.polarity = 'high')} title="Above the level">Above</button>
          <button class:on={trig.polarity === 'low'} onclick={() => (trig.polarity = 'low')} title="Below the level">Below</button>
        </div>
      </div>
    {/if}

    {#if trig.kind === 'window'}
      <div class="row">
        <label>Low <input use:siInput={{ get: () => trig.low, set: (v) => (trig.low = v), unit, v: trig.low }} /></label>
        <label>High <input use:siInput={{ get: () => trig.high, set: (v) => (trig.high = v), unit, v: trig.high }} /></label>
      </div>
    {:else}
      <div class="row">
        <label>Level <input use:siInput={{ get: () => trig.level, set: (v) => (trig.level = v), unit, v: trig.level }} /></label>
        <button class="ghost small fifty" onclick={level50} title="Halfway between the screen's min and max">50%</button>
      </div>
    {/if}

    {#if trig.kind === 'pulse'}
      <label>
        Width
        <select bind:value={trig.cond}>
          <option value="any">any</option>
          <option value="gt">longer than</option>
          <option value="lt">shorter than</option>
          <option value="within">between</option>
        </select>
      </label>
      {#if trig.cond !== 'any'}
        <div class="row">
          {#if trig.cond !== 'lt'}
            <label>{trig.cond === 'within' ? 'Min' : 'Width >'} <input use:siInput={{ get: () => trig.minWidth, set: (v) => (trig.minWidth = Math.max(0, v)), unit: 's' }} /></label>
          {/if}
          {#if trig.cond !== 'gt'}
            <label>{trig.cond === 'within' ? 'Max' : 'Width <'} <input use:siInput={{ get: () => trig.maxWidth, set: (v) => (trig.maxWidth = Math.max(0, v)), unit: 's' }} /></label>
          {/if}
        </div>
      {/if}
    {:else if trig.kind === 'timeout'}
      <label title="Fires when the signal has stayed on its side of the level this long">
        For longer than
        <input use:siInput={{ get: () => trig.timeout, set: (v) => (trig.timeout = Math.max(0, v)), unit: 's' }} />
      </label>
    {/if}

    <details class="more">
      <summary>Noise, holdoff, filter</summary>
      <div class="row">
        <label title="Re-arm only after the signal has gone this far back past the level: noise on the level doesn't fire it again and again">
          Noise reject
          <input use:siInput={{ get: () => trig.hysteresis, set: (v) => (trig.hysteresis = Math.abs(v)), unit }} />
        </label>
        <label title="Minimum time between triggers">
          Holdoff
          <input use:siInput={{ get: () => trig.holdoff, set: (v) => (trig.holdoff = Math.max(0, v)), unit: 's' }} />
        </label>
      </div>
      <label title="Trigger on a moving average of the source over this time (its delay is taken out of the event times); 0: off">
        Filter (moving average)
        <input use:siInput={{ get: () => trig.filter, set: (v) => (trig.filter = Math.max(0, v)), unit: 's' }} />
      </label>
    </details>

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
