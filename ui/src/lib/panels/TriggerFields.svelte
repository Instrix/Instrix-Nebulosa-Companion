<script lang="ts">
  // The trigger's own settings (source, type and its settings, noise / holdoff
  // / filter): the Scope's Trigger section and the Logger's triggered capture
  // share them (one trigger in the core).
  import type { Signal } from '../nbp';
  import { SIGNAL_INFO } from '../Scope.svelte';
  import { siInput } from '../siInput';
  import { getContext } from 'svelte';
  import { KIND_LABEL, MEASURE_LABEL, type KindType, type Measure, type TriggerState } from '../trigger.svelte';
  import { isNative } from '../device';
  import TriggerScript from './TriggerScript.svelte';

  let {
    trig,
    available,
    onlevel50,
    picking = false,
    onpick,
  }: {
    trig: TriggerState;
    available: Signal[];
    /** the level halfway between the shown min and max */
    onlevel50: () => void;
    /** set the level by clicking a plot: on, and the toggle */
    picking?: boolean;
    onpick?: () => void;
  } = $props();
  const unit = $derived(SIGNAL_INFO[trig.source].unit);
  /** a pulse's limits: in seconds, in the source's unit, or its integral's */
  const limUnit = $derived(trig.measure === 'width' ? 's' : trig.measure === 'charge' ? (SIGNAL_INFO[trig.source].integral ?? `${unit}·s`) : unit);
  const timed = $derived(trig.kind === 'slew' || trig.kind === 'interval' || (trig.kind === 'pulse' && trig.measure === 'width'));
  const condWhat = $derived(trig.kind === 'slew' ? 'Time' : trig.kind === 'interval' ? 'Interval' : MEASURE_LABEL[trig.measure]);
  /** in a popped-out window (Section.svelte): room for the sketch, the extras unfolded */
  const floating = getContext<(() => boolean) | undefined>('section-floating') ?? (() => false);

  /** a sketch of what the trigger fires on: the signal, the level(s) dashed, ● where it fires */
  const sketch = $derived.by(() => {
    const lv = 30;
    switch (trig.kind) {
      case 'edge': {
        const rise = 'M4,48 L70,48 L84,12 L150,12 L164,48 L216,48';
        const fires = trig.slope === 'rise' ? [[77, lv]] : trig.slope === 'fall' ? [[157, lv]] : [[77, lv], [157, lv]];
        return { path: rise, levels: [lv], fires, note: trig.slope === 'rise' ? 'fires as it rises through the level' : trig.slope === 'fall' ? 'fires as it falls through the level' : 'fires either way through the level', span: null };
      }
      case 'window': {
        const fires = trig.when === 'enter' ? [[60, 22], [168, 38]] : trig.when === 'exit' ? [[96, 22], [196, 38]] : [[60, 22], [96, 22], [168, 38], [196, 38]];
        return { path: 'M4,8 L50,8 L68,30 L90,30 L104,8 L150,8 L160,52 L190,52 L200,30 L216,30', levels: [22, 38], fires, note: trig.when === 'enter' ? 'fires entering the band' : trig.when === 'exit' ? 'fires leaving the band' : 'fires entering and leaving the band', span: null };
      }
      case 'pulse': {
        const hi = trig.polarity === 'high';
        const p = hi ? 'M4,48 L70,48 L74,10 L140,10 L144,48 L216,48' : 'M4,10 L70,10 L74,50 L140,50 L144,10 L216,10';
        const cond = { any: 'any width', gt: 'longer than', lt: 'shorter than', within: 'between two widths' }[trig.cond];
        return { path: p, levels: [lv], fires: [[142, lv]], note: `fires as a pulse ${hi ? 'above' : 'below'} the level ends: ${cond}`, span: [72, 142] as [number, number] };
      }
      case 'timeout': {
        const hi = trig.polarity === 'high';
        const p = hi ? 'M4,48 L40,48 L44,12 L216,12' : 'M4,12 L40,12 L44,50 L216,50';
        return { path: p, levels: [lv], fires: [[150, hi ? 12 : 50]], note: `fires when it has stayed ${hi ? 'above' : 'below'} the level this long`, span: [42, 150] as [number, number] };
      }
      case 'runt': {
        const hi = trig.polarity === 'high';
        // a full pulse past both levels, then one that turns back between them
        const p = hi ? 'M4,52 L30,52 L36,8 L70,8 L76,52 L120,52 L128,26 L150,26 L158,52 L216,52' : 'M4,8 L30,8 L36,52 L70,52 L76,8 L120,8 L128,34 L150,34 L158,8 L216,8';
        return { path: p, levels: [18, 42], fires: [[156, hi ? 42 : 18]], note: `fires on a pulse ${hi ? 'over the low level that turns back before the high one' : 'under the high level that turns back before the low one'}`, span: null };
      }
      case 'slew': {
        const fall = trig.slope === 'fall';
        const p = 'M4,52 L40,52 L90,8 L130,8 L180,52 L216,52';
        const rise = [[78, 18]], fallAt = [[168, 42]];
        const fires = trig.slope === 'rise' ? rise : fall ? fallAt : [...rise, ...fallAt];
        return { path: p, levels: [18, 42], fires, note: `times the ${trig.slope === 'both' ? 'rise and fall' : trig.slope} between the two levels`, span: (fall ? [142, 168] : [51, 78]) as [number, number] };
      }
      case 'interval': {
        const p = 'M4,48 L30,48 L34,12 L60,12 L64,48 L120,48 L124,12 L150,12 L154,48 L216,48';
        return { path: p, levels: [lv], fires: [[122, lv]], note: 'fires on an edge, by the time since the one before', span: [32, 122] as [number, number] };
      }
    }
    return { path: '', levels: [], fires: [], note: '', span: null };
  });
</script>

{#if floating() && trig.kind !== 'script'}
  <figure class="sketch" aria-label="What this trigger fires on">
    <svg viewBox="0 0 220 60" width="100%" height="60" role="img">
      {#each sketch.levels as y (y)}<line x1="0" x2="220" y1={y} y2={y} class="lvl" />{/each}
      {#if sketch.span}
        <line x1={sketch.span[0]} x2={sketch.span[1]} y1="56" y2="56" class="span" />
        <line x1={sketch.span[0]} x2={sketch.span[0]} y1="52" y2="60" class="span" />
        <line x1={sketch.span[1]} x2={sketch.span[1]} y1="52" y2="60" class="span" />
      {/if}
      <path d={sketch.path} class="sig" style="stroke: var({SIGNAL_INFO[trig.source].color})" />
      {#each sketch.fires as [x, y], k (k)}<circle cx={x} cy={y} r="4" class="fire" />{/each}
    </svg>
    <figcaption>{SIGNAL_INFO[trig.source].name}: {sketch.note}</figcaption>
  </figure>
{/if}

  {#if trig.kind !== 'script'}
  <div class="axis">
    <span>Source</span>
    <div class="seg">
      {#each available as sg (sg)}
        <button class:on={trig.source === sg} onclick={() => (trig.source = sg)} style="color: {trig.source === sg ? '' : `var(${SIGNAL_INFO[sg].color})`}">{SIGNAL_INFO[sg].sym}</button>
      {/each}
    </div>
  </div>
  {/if}
  <label>
    Type
    <select bind:value={trig.kind}>
      {#each Object.entries(KIND_LABEL).filter(([k]) => k !== 'script' || isNative || trig.kind === 'script') as [k, label] (k)}<option value={k as KindType}>{label}</option>{/each}
    </select>
  </label>

  {#if trig.kind === 'script'}
    {#if isNative}
      <TriggerScript {trig} />
    {:else}
      <p class="note">A script trigger runs Python: it needs the desktop app. Choose another type here.</p>
    {/if}
  {:else}

  {#if trig.kind === 'edge' || trig.kind === 'slew' || trig.kind === 'interval'}
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
      <span>{trig.kind === 'timeout' ? 'Stays' : 'Pulse'}</span>
      <div class="seg">
        <button class:on={trig.polarity === 'high'} onclick={() => (trig.polarity = 'high')} title="Above the level">Above</button>
        <button class:on={trig.polarity === 'low'} onclick={() => (trig.polarity = 'low')} title="Below the level">Below</button>
      </div>
    </div>
  {/if}

  {#if trig.kind === 'window' || trig.kind === 'runt' || trig.kind === 'slew'}
    <div class="row">
      <label>Low <input use:siInput={{ get: () => trig.low, set: (v) => (trig.low = v), unit, v: trig.low }} /></label>
      <label>High <input use:siInput={{ get: () => trig.high, set: (v) => (trig.high = v), unit, v: trig.high }} /></label>
    </div>
  {:else}
    <div class="row">
      <label>Level <input use:siInput={{ get: () => trig.level, set: (v) => (trig.level = v), unit, v: trig.level }} /></label>
      <button class="ghost small fifty" onclick={onlevel50} title="Halfway between the shown min and max">50%</button>
      {#if onpick}
        <button class="ghost small fifty" class:on={picking} onclick={onpick} title="Click a plot to set the source and level there">⌖</button>
      {/if}
    </div>
  {/if}

  {#if trig.kind === 'pulse'}
    <label title="What the condition looks at: the pulse's width, its charge (∫ dt: C for current, J for power), its peak or its mean">
      Measure
      <select bind:value={trig.measure}>
        {#each Object.entries(MEASURE_LABEL) as [k, label] (k)}<option value={k as Measure}>{label}</option>{/each}
      </select>
    </label>
  {/if}
  {#if trig.kind === 'pulse' || trig.kind === 'slew' || trig.kind === 'interval'}
    <label>
      {condWhat}
      <select bind:value={trig.cond}>
        <option value="any">any</option>
        <option value="gt">{timed ? 'longer than' : 'more than'}</option>
        <option value="lt">{timed ? 'shorter than' : 'less than'}</option>
        <option value="within">between</option>
      </select>
    </label>
    {#if trig.cond !== 'any'}
      <div class="row">
        {#if timed}
          {#if trig.cond !== 'lt'}
            <label>{trig.cond === 'within' ? 'Min' : `${condWhat} >`} <input use:siInput={{ get: () => trig.minWidth, set: (v) => (trig.minWidth = Math.max(0, v)), unit: 's', v: trig.minWidth }} /></label>
          {/if}
          {#if trig.cond !== 'gt'}
            <label>{trig.cond === 'within' ? 'Max' : `${condWhat} <`} <input use:siInput={{ get: () => trig.maxWidth, set: (v) => (trig.maxWidth = Math.max(0, v)), unit: 's', v: trig.maxWidth }} /></label>
          {/if}
        {:else}
          {#if trig.cond !== 'lt'}
            <label>{trig.cond === 'within' ? 'Min' : `${condWhat} >`} <input use:siInput={{ get: () => trig.minVal, set: (v) => (trig.minVal = v), unit: limUnit, v: trig.minVal }} /></label>
          {/if}
          {#if trig.cond !== 'gt'}
            <label>{trig.cond === 'within' ? 'Max' : `${condWhat} <`} <input use:siInput={{ get: () => trig.maxVal, set: (v) => (trig.maxVal = v), unit: limUnit, v: trig.maxVal }} /></label>
          {/if}
        {/if}
      </div>
    {/if}
  {:else if trig.kind === 'timeout'}
    <label title="Fires when the signal has stayed on its side of the level this long">
      For longer than
      <input use:siInput={{ get: () => trig.timeout, set: (v) => (trig.timeout = Math.max(0, v)), unit: 's' }} />
    </label>
  {/if}

  <details class="more" open={floating() || undefined}>
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

  <details class="more" open={floating() || trig.extras() !== '' || undefined}>
    <summary>Sequence, qualifier, every Nth{trig.extras() ? ` · ${trig.extras()}` : ''}</summary>
    <label class="check" title="Two stages: the trigger above (A) arms this one (B); B's edge is the trigger">
      <input type="checkbox" bind:checked={trig.seqOn} /> Then B: an edge after A
    </label>
    {#if trig.seqOn}
      <div class="axis">
        <span>B on</span>
        <div class="seg">
          {#each available as sg (sg)}
            <button class:on={trig.seqSource === sg} onclick={() => (trig.seqSource = sg)} style="color: {trig.seqSource === sg ? '' : `var(${SIGNAL_INFO[sg].color})`}">{SIGNAL_INFO[sg].sym}</button>
          {/each}
        </div>
        <div class="seg">
          <button class:on={trig.seqSlope === 'rise'} onclick={() => (trig.seqSlope = 'rise')} title="Rising">↑</button>
          <button class:on={trig.seqSlope === 'fall'} onclick={() => (trig.seqSlope = 'fall')} title="Falling">↓</button>
          <button class:on={trig.seqSlope === 'both'} onclick={() => (trig.seqSlope = 'both')} title="Either">↕</button>
        </div>
      </div>
      <label>B level <input use:siInput={{ get: () => trig.seqLevel, set: (v) => (trig.seqLevel = v), unit: SIGNAL_INFO[trig.seqSource].unit, v: trig.seqLevel }} /></label>
      <div class="row">
        <label title="B counts only this long after A at least">After ≥ <input use:siInput={{ get: () => trig.seqDelay, set: (v) => (trig.seqDelay = Math.max(0, v)), unit: 's', v: trig.seqDelay }} /></label>
        <label title="B must come within this long of A (0: any time, until the next A)">Within <input use:siInput={{ get: () => trig.seqWithin, set: (v) => (trig.seqWithin = Math.max(0, v)), unit: 's', v: trig.seqWithin }} /></label>
      </div>
    {/if}

    <label class="check" title="Fire only while another signal is above or below a level (checked at the trigger)">
      <input type="checkbox" bind:checked={trig.qualOn} /> Only while
    </label>
    {#if trig.qualOn}
      <div class="axis">
        <div class="seg">
          {#each available as sg (sg)}
            <button class:on={trig.qualSource === sg} onclick={() => (trig.qualSource = sg)} style="color: {trig.qualSource === sg ? '' : `var(${SIGNAL_INFO[sg].color})`}">{SIGNAL_INFO[sg].sym}</button>
          {/each}
        </div>
        <div class="seg">
          <button class:on={trig.qualPolarity === 'high'} onclick={() => (trig.qualPolarity = 'high')}>≥</button>
          <button class:on={trig.qualPolarity === 'low'} onclick={() => (trig.qualPolarity = 'low')}>≤</button>
        </div>
        <input class="qlevel" aria-label="Qualifier level" use:siInput={{ get: () => trig.qualLevel, set: (v) => (trig.qualLevel = v), unit: SIGNAL_INFO[trig.qualSource].unit, v: trig.qualLevel }} />
      </div>
    {/if}

    <label title="Keep only every Nth trigger (1: all)">
      Every Nth
      <input type="number" min="1" step="1" value={trig.every} onchange={(e) => (trig.every = Math.max(1, Math.round(+e.currentTarget.value) || 1))} />
    </label>
  </details>
  {/if}

<style>
  .sketch {
    margin: 0;
    padding: 6px 8px;
    border: 1px solid var(--border);
    border-radius: var(--r);
    background: var(--scope-bg, var(--bg));
  }
  .sketch figcaption {
    font-size: 11px;
    color: var(--muted);
    margin-top: 2px;
  }
  .sig {
    fill: none;
    stroke-width: 2;
  }
  .lvl {
    stroke: var(--trig);
    stroke-dasharray: 5 4;
    opacity: 0.8;
  }
  .span {
    stroke: var(--muted);
  }
  .fire {
    fill: var(--danger);
  }
  .note {
    margin: 0;
    font-size: 12px;
    color: var(--muted);
  }
  .qlevel {
    flex: 1;
    min-width: 0;
  }
  .fifty.on {
    color: var(--accent);
    border-color: var(--accent);
  }
</style>
