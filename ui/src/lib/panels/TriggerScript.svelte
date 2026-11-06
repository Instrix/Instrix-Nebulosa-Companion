<script lang="ts">
  // The Script trigger type (desktop app): a Python function that gets the
  // samples in blocks as they come and says where the triggers are. The code
  // is edited as a draft and applied.
  import { SCRIPT_EXAMPLE, type TriggerState } from '../trigger.svelte';

  let { trig }: { trig: TriggerState } = $props();

  // svelte-ignore state_referenced_locally
  let draft = $state(trig.scriptCode);
  const dirty = $derived(draft !== trig.scriptCode);
  let file: HTMLInputElement | undefined = $state();
  let checking = $state(false);

  async function apply() {
    trig.scriptCode = draft;
    checking = true;
    await trig.loadScript();
    checking = false;
  }

  function keydown(e: KeyboardEvent) {
    // Ctrl / ⌘ + Enter applies; Tab indents (it's Python)
    if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      void apply();
    } else if (e.key === 'Tab' && !e.shiftKey) {
      e.preventDefault();
      const el = e.currentTarget as HTMLTextAreaElement;
      const { selectionStart: a, selectionEnd: b } = el;
      draft = draft.slice(0, a) + '    ' + draft.slice(b);
      requestAnimationFrame(() => el.setSelectionRange(a + 4, a + 4));
    }
    e.stopPropagation(); // the app's keys (Space, arrows) aren't for here
  }

  async function open(e: Event) {
    const f = (e.currentTarget as HTMLInputElement).files?.[0];
    if (f) draft = await f.text();
    (e.currentTarget as HTMLInputElement).value = '';
  }

  function save() {
    const a = document.createElement('a');
    a.href = URL.createObjectURL(new Blob([draft], { type: 'text/x-python' }));
    a.download = 'trigger.py';
    a.click();
    setTimeout(() => URL.revokeObjectURL(a.href), 2000);
  }

  const r = $derived(trig.script);
  const fmtSamples = (n: number) => (n >= 1e9 ? `${(n / 1e9).toFixed(2)} G` : n >= 1e6 ? `${(n / 1e6).toFixed(1)} M` : n >= 1e3 ? `${(n / 1e3).toFixed(0)} k` : String(n));
</script>

<div class="script">
  <textarea class="code" spellcheck="false" rows="14" bind:value={draft} onkeydown={keydown} aria-label="Trigger script (Python)"></textarea>
  <div class="row buttons">
    <button class="primary small" onclick={apply} disabled={checking} title="Use this code (Ctrl + Enter)">{dirty ? 'Apply' : 'Reload'}</button>
    <button class="ghost small" onclick={() => file?.click()} title="Open a .py file">Open…</button>
    <button class="ghost small" onclick={save} title="Save the code as a .py file">Save…</button>
    <button class="ghost small" onclick={() => (draft = SCRIPT_EXAMPLE)} title="Put back the example">Example</button>
    <input bind:this={file} type="file" accept=".py,text/x-python,text/plain" hidden onchange={open} />
  </div>
  <label title="The Python interpreter: empty for python3 (python on Windows), or a path, e.g. a virtual environment's">
    Python
    <input placeholder="python3" bind:value={trig.python} onchange={apply} spellcheck="false" />
  </label>

  <p class="status" aria-live="polite">
    {#if checking}
      Starting Python…
    {:else if r.info}
      Python {r.info.python}{r.info.numpy ? ` · numpy ${r.info.numpy}` : ' · no numpy (lists)'}
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
    {#if dirty}<br /><span class="muted">Edited: Apply to use it.</span>{/if}
  </p>
  {#if r.error}<pre class="err">{r.error}</pre>{/if}
  {#if r.out}
    <details>
      <summary>Printed</summary>
      <pre class="out">{r.out}</pre>
    </details>
  {/if}
</div>

<style>
  .script {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .code {
    width: 100%;
    box-sizing: border-box;
    font: 12px/1.45 ui-monospace, monospace;
    tab-size: 4;
    resize: vertical;
    white-space: pre;
    overflow: auto;
    min-height: 10em;
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
  .err {
    color: var(--danger);
    border: 1px solid var(--danger);
  }
</style>
