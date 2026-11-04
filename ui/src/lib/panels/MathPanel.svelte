<script lang="ts">
  // The math channel: an expression of I, V, P, t computed on every sample, its name and unit.
  import { mathInfo } from '../Scope.svelte';
  import Section from './Section.svelte';

  export interface MathDraft {
    name: string;
    unit: string;
    expr: string;
  }

  let {
    draft = $bindable(),
    error,
    onapply,
    onoff,
  }: {
    draft: MathDraft;
    error: string;
    onapply: () => void;
    onoff: () => void;
  } = $props();

  const MATH_EXAMPLES = [
    { label: 'Power in mW', expr: 'P*1000', name: 'Power', unit: 'mW' },
    { label: 'Load resistance', expr: 'V/I', name: 'Load', unit: 'Ω' },
    { label: 'Charge since start (mAh)', expr: 'integ(I)/3.6', name: 'Charge', unit: 'mAh' },
    { label: 'Energy since start (mWh)', expr: 'integ(P)/3.6', name: 'Energy', unit: 'mWh' },
    { label: 'Current, 1 ms average', expr: 'avg(I, 1ms)', name: 'I avg 1 ms', unit: 'A' },
    { label: 'dI/dt', expr: 'ddt(I)', name: 'dI/dt', unit: 'A/s' },
    { label: 'Current in µA, log', expr: 'log10(I/1u)', name: 'log I', unit: 'dec µA' },
  ];

  function pickMathExample(expr: string) {
    const ex = MATH_EXAMPLES.find((x) => x.expr === expr);
    if (ex) draft = { name: ex.name, unit: ex.unit, expr: ex.expr };
  }
</script>

<Section id="math" title="Math channel" open={false} summary={mathInfo.on ? `${mathInfo.name} = ${draft.expr}` : 'off'}>
    <div class="cal">
      <label>
        Example
        <select onchange={(e) => pickMathExample((e.currentTarget as HTMLSelectElement).value)}>
          <option value="">Choose…</option>
          {#each MATH_EXAMPLES as ex (ex.expr)}<option value={ex.expr}>{ex.label}</option>{/each}
        </select>
      </label>
      <label title="I (A), V (V), P (W), t (s); + - * / ^; abs sqrt exp ln log10 min max; avg(x, 1ms) integ(x) ddt(x); numbers with SI prefixes (10m, 1u)">
        Expression
        <input class="mono" bind:value={draft.expr} placeholder="P*1000" onkeydown={(e) => e.key === 'Enter' && onapply()} />
      </label>
      <div class="row">
        <label>Name <input bind:value={draft.name} placeholder="Math" /></label>
        <label>Unit <input bind:value={draft.unit} placeholder="mW" /></label>
      </div>
      <div class="row">
        <button class="primary" onclick={onapply}>{mathInfo.on ? 'Update' : 'Apply'}</button>
        {#if mathInfo.on}<button class="ghost" onclick={onoff}>Off</button>{/if}
      </div>
      {#if error}<p class="error small">{error}</p>{/if}
      <p class="muted small">Computed on every sample, as the 4th signal: plots, stats, markers, triggers, analysis and CSV.</p>
    </div>
</Section>
