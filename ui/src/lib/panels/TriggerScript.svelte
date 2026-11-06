<script lang="ts">
  // The Script trigger type in the panel (desktop app): which script runs, how
  // it's doing, and the editor (a dialog of its own) to write one.
  import { scriptName } from '../scriptExamples';
  import type { TriggerState } from '../trigger.svelte';
  import ScriptEditor from './ScriptEditor.svelte';

  let { trig }: { trig: TriggerState } = $props();

  let editing = $state(false);
  let checking = $state(false);

  async function reload() {
    checking = true;
    await trig.loadScript();
    checking = false;
  }

  const r = $derived(trig.script);
  const name = $derived(scriptName(trig.scriptCode));
  const fmtSamples = (n: number) => (n >= 1e9 ? `${(n / 1e9).toFixed(2)} G` : n >= 1e6 ? `${(n / 1e6).toFixed(1)} M` : n >= 1e3 ? `${(n / 1e3).toFixed(0)} k` : String(n));
  /** the error's first and last lines: the whole traceback is in the editor */
  const short = $derived(r.error ? r.error.split('\n').filter((l) => l.trim()).slice(-2).join('\n') : '');
</script>

<div class="script">
  <div class="which">
    <span class="name" title="The script the trigger runs">{name}</span>
    <span class="muted">{trig.scriptCode.split('\n').length} lines{trig.scriptDraft !== trig.scriptCode ? ' · a draft in the editor' : ''}</span>
  </div>
  <div class="row buttons">
    <button class="primary small" onclick={() => (editing = true)} title="Write the script, try examples, check it">Edit script…</button>
    <button class="ghost small" onclick={reload} disabled={checking} title="Start the script's Python again (its globals start over)">Restart</button>
  </div>
  <label title="The Python interpreter: empty for python3 (python on Windows), or a path, e.g. a virtual environment's">
    Python
    <input placeholder="python3" bind:value={trig.python} onchange={reload} spellcheck="false" />
  </label>

  <p class="status" aria-live="polite">
    {#if checking}
      Starting Python…
    {:else if r.info}
      Python {r.info.python}{r.info.numpy ? ` · numpy ${r.info.numpy}` : ' · no numpy'}
    {/if}
    {#if r.blocks || r.failed}
      <br /><b>{r.found.toLocaleString()} found</b> in {fmtSamples(r.samples)} samples
      {#if r.failed}· <span class="bad">{r.failed} failed</span>{/if}
      · {r.msPerM.toFixed(0)} ms per million
      {#if r.behind > 0.5}· <span class="bad">{r.behind.toFixed(1)} s behind</span>{/if}
      {#if r.skipped}· <span class="bad" title="The script couldn't keep up: these samples weren't looked at">{r.skipped.toFixed(1)} s skipped</span>{/if}
    {:else if trig.detecting && !r.error}
      <br />Waiting for samples…
    {:else if !trig.detecting}
      <br /><span class="muted">Runs while the trigger detects (Auto, Normal, Single, Log events, or the Logger watching).</span>
    {/if}
  </p>
  {#if r.error}<button class="err" onclick={() => (editing = true)} title="Open the editor: the whole message, and the line">{short}</button>{/if}
  {#if r.out}
    <details>
      <summary>Printed</summary>
      <pre class="out">{r.out}</pre>
    </details>
  {/if}
</div>

{#if editing}
  <ScriptEditor {trig} onclose={() => (editing = false)} />
{/if}

<style>
  .script {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .which {
    display: flex;
    flex-direction: column;
    padding: 6px 8px;
    border: 1px solid var(--border);
    border-radius: var(--r);
    font-size: 12px;
  }
  .name {
    font-weight: 600;
  }
  .buttons {
    flex-wrap: wrap;
  }
  .status {
    margin: 0;
    font-size: 11px;
    color: var(--muted);
  }
  .bad {
    color: var(--danger);
  }
  .err {
    text-align: left;
    white-space: pre-wrap;
    font: 11px/1.4 ui-monospace, monospace;
    color: var(--danger);
    border: 1px solid var(--danger);
    background: none;
    border-radius: var(--r);
    padding: 6px;
    cursor: pointer;
  }
  pre {
    margin: 0;
    max-height: 12em;
    overflow: auto;
    font: 11px/1.4 ui-monospace, monospace;
    padding: 6px;
    border-radius: var(--r);
    background: var(--scope-bg, var(--bg));
    white-space: pre-wrap;
  }
</style>
