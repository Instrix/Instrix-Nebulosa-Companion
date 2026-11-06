<script lang="ts">
  // The trigger script's editor: a dialog of its own, room to write Python.
  // Examples to start from, scripts saved by name on this computer, .py files
  // opened and saved, Check (it loads, and a dry run on the last second of
  // samples: what it would find), Apply. The draft lives in the trigger's
  // state, so closing keeps it.
  import { ask } from '../ask';
  import { isNative } from '../device';
  import * as native from '../native';
  import { errorLine, type CheckResult } from '../script';
  import { SCRIPT_EXAMPLES, scriptName } from '../scriptExamples';
  import type { TriggerState } from '../trigger.svelte';
  import { fmtTime } from '../view.svelte';

  let { trig, onclose }: { trig: TriggerState; onclose: () => void } = $props();

  const LIB_KEY = 'nebulosa.triggerScripts';
  function readLib(): Record<string, string> {
    try {
      const v = JSON.parse(localStorage.getItem(LIB_KEY) ?? '{}');
      return v && typeof v === 'object' ? v : {};
    } catch {
      return {};
    }
  }
  let lib = $state(readLib());
  function writeLib() {
    try {
      localStorage.setItem(LIB_KEY, JSON.stringify(lib));
    } catch {
      /* storage unavailable */
    }
  }

  let ta: HTMLTextAreaElement | undefined = $state();
  let gutter: HTMLDivElement | undefined = $state();
  let file: HTMLInputElement | undefined = $state();
  let menu = $state<'examples' | 'saved' | null>(null);
  let check = $state<CheckResult | null>(null);
  let checking = $state(false);
  let applying = $state(false);
  let note = $state('');
  let showRef = $state(false);
  /** the name it was last saved under (in the list), for Save again */
  let savedAs = $state<string | null>(Object.entries(readLib()).find(([, c]) => c === trig.scriptDraft)?.[0] ?? null);

  const lines = $derived(trig.scriptDraft.split('\n').length);
  const applied = $derived(trig.scriptDraft === trig.scriptCode);
  const badLine = $derived(check?.error ? errorLine(check.error) : null);
  let caret = $state({ line: 1, col: 1 });

  function syncScroll() {
    if (gutter && ta) gutter.scrollTop = ta.scrollTop;
  }
  function trackCaret() {
    if (!ta) return;
    const before = ta.value.slice(0, ta.selectionStart);
    const ls = before.split('\n');
    caret = { line: ls.length, col: ls[ls.length - 1].length + 1 };
  }

  /** puts the caret at the start of line n, in view */
  function gotoLine(n: number) {
    if (!ta) return;
    const ls = trig.scriptDraft.split('\n');
    const at = ls.slice(0, n - 1).reduce((a, l) => a + l.length + 1, 0);
    ta.focus();
    ta.setSelectionRange(at, at + (ls[n - 1]?.length ?? 0));
    const lh = parseFloat(getComputedStyle(ta).lineHeight) || 18;
    ta.scrollTop = Math.max(0, (n - 4) * lh);
    syncScroll();
    trackCaret();
  }

  /** replaces [a, b) with text, the caret after it (undo works: execCommand keeps the history) */
  function insert(a: number, b: number, text: string, caretAt?: number) {
    if (!ta) return;
    ta.setSelectionRange(a, b);
    if (!document.execCommand('insertText', false, text)) {
      trig.scriptDraft = ta.value.slice(0, a) + text + ta.value.slice(b);
    } else trig.scriptDraft = ta.value;
    const c = caretAt ?? a + text.length;
    requestAnimationFrame(() => ta?.setSelectionRange(c, c));
  }

  function keydown(e: KeyboardEvent) {
    e.stopPropagation(); // the app's keys aren't for here
    if (!ta) return;
    const { selectionStart: a, selectionEnd: b, value } = ta;
    const mod = e.ctrlKey || e.metaKey;
    if (mod && e.key === 'Enter') {
      e.preventDefault();
      void apply(e.shiftKey);
    } else if (mod && (e.key === 's' || e.key === 'S')) {
      e.preventDefault();
      void saveNamed();
    } else if (mod && e.key === '/') {
      // comment / uncomment the lines selected
      e.preventDefault();
      const s0 = value.lastIndexOf('\n', a - 1) + 1;
      const s1 = value.indexOf('\n', b - (b > a && value[b - 1] === '\n' ? 1 : 0));
      const end = s1 < 0 ? value.length : s1;
      const block = value.slice(s0, end).split('\n');
      const off = block.every((l) => !l.trim() || /^\s*#/.test(l));
      const out = block.map((l) => (!l.trim() ? l : off ? l.replace(/^(\s*)# ?/, '$1') : l.replace(/^(\s*)/, '$1# '))).join('\n');
      insert(s0, end, out, s0 + out.length);
    } else if (e.key === 'Tab') {
      e.preventDefault();
      const s0 = value.lastIndexOf('\n', a - 1) + 1;
      if (a === b && !e.shiftKey) insert(a, b, '    ');
      else {
        // indent / dedent the lines selected
        const end = value.indexOf('\n', b - 1) < 0 ? value.length : value.indexOf('\n', b - 1) === b - 1 ? b - 1 : value.indexOf('\n', b - 1);
        const block = value.slice(s0, end).split('\n');
        const out = block.map((l) => (e.shiftKey ? l.replace(/^ {1,4}/, '') : '    ' + l)).join('\n');
        insert(s0, end, out, s0 + out.length);
      }
    } else if (e.key === 'Enter' && !mod) {
      // keep the indent; one more after a colon
      e.preventDefault();
      const s0 = value.lastIndexOf('\n', a - 1) + 1;
      const line = value.slice(s0, a);
      let ind = line.match(/^\s*/)![0];
      if (/:\s*(#.*)?$/.test(line)) ind += '    ';
      insert(a, b, '\n' + ind);
    } else if (e.key === 'Backspace' && a === b && a > 0) {
      // a whole indent step at once
      const s0 = value.lastIndexOf('\n', a - 1) + 1;
      const before = value.slice(s0, a);
      if (before.length && /^ +$/.test(before)) {
        e.preventDefault();
        const n = before.length % 4 || 4;
        insert(a - n, a, '');
      }
    } else if (e.key === 'Escape') {
      e.preventDefault();
      void close();
    }
  }

  async function confirmLose(what: string) {
    return applied || trig.scriptDraft === lib[savedAs ?? ''] || (await ask(`The script in the editor isn't applied or saved. ${what} anyway?`));
  }

  async function useCode(code: string, name: string | null) {
    trig.scriptDraft = code;
    savedAs = name;
    check = null;
    menu = null;
    requestAnimationFrame(() => {
      if (ta) ta.scrollTop = 0;
      syncScroll();
    });
  }

  async function pickExample(code: string) {
    if (!(await confirmLose('Replace it with the example'))) return;
    await useCode(code, null);
  }
  async function pickSaved(name: string) {
    if (!(await confirmLose(`Open “${name}”`))) return;
    await useCode(lib[name], name);
  }
  async function deleteSaved(name: string) {
    if (!(await ask(`Delete the saved script “${name}”?`))) return;
    delete lib[name];
    lib = { ...lib };
    writeLib();
    if (savedAs === name) savedAs = null;
  }

  /** the name being typed for Save as (null: not asking) */
  let naming = $state<string | null>(null);
  let nameEl: HTMLInputElement | undefined = $state();

  /** into the list of saved scripts, under a name (Ctrl + S: again under the same one) */
  async function saveNamed(rename = false) {
    if (!savedAs || rename) {
      naming = savedAs ?? (scriptName(trig.scriptDraft) === 'Custom script' ? '' : `My ${scriptName(trig.scriptDraft).toLowerCase()}`);
      requestAnimationFrame(() => nameEl?.select());
      return;
    }
    store(savedAs);
  }

  async function confirmName() {
    const name = naming?.trim();
    if (!name) return;
    if (lib[name] != null && name !== savedAs && !(await ask(`There's a script “${name}” already. Replace it?`))) return;
    naming = null;
    store(name);
  }

  function store(name: string) {
    lib = { ...lib, [name]: trig.scriptDraft };
    writeLib();
    savedAs = name;
    flash(`Saved as “${name}”`);
  }

  async function openFile(e: Event) {
    const el = e.currentTarget as HTMLInputElement;
    const f = el.files?.[0];
    el.value = '';
    if (!f || !(await confirmLose(`Open ${f.name}`))) return;
    await useCode(await f.text(), null);
    flash(`Opened ${f.name}`);
  }

  async function saveFile() {
    const bytes = new TextEncoder().encode(trig.scriptDraft);
    const base = (savedAs ?? 'trigger').replace(/[^\w.-]+/g, '-') + '.py';
    if (isNative) {
      const path = await native.pickSave(base, 'Python', 'py');
      if (!path) return;
      await native.saveBytes(path, bytes);
      flash(`Saved ${path}`);
    } else {
      const a = document.createElement('a');
      a.href = URL.createObjectURL(new Blob([bytes], { type: 'text/x-python' }));
      a.download = base;
      a.click();
      setTimeout(() => URL.revokeObjectURL(a.href), 2000);
    }
  }

  async function runCheck() {
    checking = true;
    check = null;
    check = await trig.checkScript(trig.scriptDraft);
    checking = false;
    if (badLine) gotoLine(badLine);
  }

  async function apply(andClose = false) {
    applying = true;
    const ok = await trig.applyScript(trig.scriptDraft);
    applying = false;
    if (!ok) {
      check = { info: null, error: trig.script.error, n: 0, span: 0, found: 0, events: [], out: '', ms: 0 };
      if (badLine) gotoLine(badLine);
      return;
    }
    flash('Applied: the trigger runs this script now');
    if (andClose) onclose();
  }

  async function close() {
    onclose();
  }

  let flashTimer: ReturnType<typeof setTimeout>;
  function flash(m: string) {
    note = m;
    clearTimeout(flashTimer);
    flashTimer = setTimeout(() => (note = ''), 3000);
  }

  function portal(node: HTMLElement) {
    document.body.appendChild(node);
    return { destroy: () => node.remove() };
  }

  const savedNames = $derived(Object.keys(lib).sort((a, b) => a.localeCompare(b)));
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<div class="sbackdrop" role="presentation" use:portal onclick={() => (menu = null)} onkeydown={(e) => e.stopPropagation()}>
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div class="sdialog" role="dialog" aria-label="Trigger script" tabindex="-1" onclick={(e) => e.stopPropagation()}>
    <header>
      <h2>Trigger script <span class="muted">· Python · {savedAs ?? scriptName(trig.scriptDraft)}{applied ? '' : ' · not applied'}</span></h2>
      <button class="ghost small" onclick={close} aria-label="Close" title="Close (Esc): the draft stays">✕</button>
    </header>

    <div class="bar">
      <div class="drop">
        <button class="ghost small" class:on={menu === 'examples'} onclick={(e) => ((menu = menu === 'examples' ? null : 'examples'), e.stopPropagation())}>Examples ▾</button>
        {#if menu === 'examples'}
          <div class="menu" role="menu">
            {#each SCRIPT_EXAMPLES as ex (ex.id)}
              <button role="menuitem" onclick={() => pickExample(ex.code)}><b>{ex.title}</b><small>{ex.about}</small></button>
            {/each}
          </div>
        {/if}
      </div>
      <div class="drop">
        <button class="ghost small" class:on={menu === 'saved'} onclick={(e) => ((menu = menu === 'saved' ? null : 'saved'), e.stopPropagation())}>Saved ({savedNames.length}) ▾</button>
        {#if menu === 'saved'}
          <div class="menu" role="menu">
            {#each savedNames as n (n)}
              <div class="saved">
                <button role="menuitem" onclick={() => pickSaved(n)}><b>{n}</b><small>{lib[n].split('\n').length} lines</small></button>
                <button class="ghost small del" onclick={() => deleteSaved(n)} title="Delete “{n}”" aria-label="Delete {n}">✕</button>
              </div>
            {:else}
              <p class="muted empty">None yet: Save keeps the script here under a name (Ctrl + S).</p>
            {/each}
          </div>
        {/if}
      </div>
      <button class="ghost small" onclick={() => saveNamed()} title="Keep it in Saved under a name (Ctrl + S)">Save</button>
      <button class="ghost small" onclick={() => saveNamed(true)} title="Save under another name">Save as…</button>
      <span class="sep"></span>
      <button class="ghost small" onclick={() => file?.click()} title="Open a .py file">Open file…</button>
      <button class="ghost small" onclick={saveFile} title="Write it to a .py file">Save file…</button>
      <input bind:this={file} type="file" accept=".py,text/x-python,text/plain" hidden onchange={openFile} />
      <span class="grow"></span>
      <button class="ghost small" class:on={showRef} onclick={() => (showRef = !showRef)}>Reference</button>
    </div>

    {#if naming != null}
      <div class="bar naming">
        <label>Save as <input bind:this={nameEl} bind:value={naming} placeholder="A name…" onkeydown={(e) => { e.stopPropagation(); if (e.key === 'Enter') void confirmName(); else if (e.key === 'Escape') naming = null; }} /></label>
        <button class="primary small" onclick={confirmName} disabled={!naming.trim()}>Save</button>
        <button class="ghost small" onclick={() => (naming = null)}>Cancel</button>
      </div>
    {/if}

    <div class="body">
      <div class="editor">
        <div class="gutter" bind:this={gutter} aria-hidden="true">
          {#each { length: lines } as _, k (k)}<div class:err={badLine === k + 1} class:cur={caret.line === k + 1}>{k + 1}</div>{/each}
        </div>
        <textarea
          bind:this={ta}
          bind:value={trig.scriptDraft}
          spellcheck="false"
          autocapitalize="off"
          autocomplete="off"
          wrap="off"
          aria-label="Trigger script (Python)"
          onkeydown={keydown}
          onscroll={syncScroll}
          onkeyup={trackCaret}
          onclick={trackCaret}
          oninput={() => (check = null)}
        ></textarea>
      </div>

      {#if showRef}
        <aside class="ref">
          <h3>trigger(data)</h3>
          <p>Gets the samples that came since the last call (a block, ~0.1–0.25 s) and returns where the triggers are:</p>
          <ul>
            <li>a list of times, s: <code>[data.t[k] for k in idx]</code></li>
            <li>(time, label) pairs: the label shows in the event list and the CSV</li>
            <li>dicts: <code>t</code>, <code>label</code>, <code>width</code>, <code>peak</code>, <code>mean</code>, <code>integral</code></li>
          </ul>
          <h3>data</h3>
          <ul>
            <li><code>t</code> times (s), <code>i</code> current (A), <code>v</code> voltage (V), <code>p</code> power (W), <code>m</code> math channel (or None): numpy arrays</li>
            <li><code>dt</code> s per sample; <code>runs</code> [(t0, count)], the stretches without a gap</li>
          </ul>
          <h3>Between blocks</h3>
          <p>Globals live on: keep the last sample, a count, a state. <code>reset()</code>, if defined, runs when the trigger starts over.</p>
          <h3>Runs()</h3>
          <p>Follows stretches where a condition holds, across blocks: <code>for r in runs.feed(data, data.i &gt; 0.005)</code> gives the ones that ended, each with <code>t0</code>, <code>t1</code>, <code>width</code>, <code>n</code>, <code>i_mean</code> / <code>i_min</code> / <code>i_max</code> (and <code>v_</code>, <code>p_</code>), <code>charge</code> (C), <code>energy</code> (J). <code>runs.current</code>: the one still going.</p>
          <h3>Also</h3>
          <p><code>np</code> (numpy), <code>math</code>; <code>print()</code> shows under Printed. Any module you have installed can be imported.</p>
        </aside>
      {/if}
    </div>

    <div class="results" aria-live="polite">
      {#if checking}
        <p>Checking: loading it, then a dry run on the latest samples…</p>
      {:else if check}
        {#if check.error && !check.info}
          <p class="bad">✗ It doesn't load{badLine ? `: line ${badLine}` : ''}</p>
        {:else if check.info}
          <p class="ok">✓ Loads: Python {check.info.python}{check.info.numpy ? `, numpy ${check.info.numpy}` : ', no numpy'}</p>
          {#if check.error}
            <p class="bad">✗ The dry run raised an error{badLine ? ` at line ${badLine}` : ''}</p>
          {:else if check.n}
            <p class="ok">✓ Dry run on the last {fmtTime(check.span)} ({(check.n / 1e6).toFixed(2)} M samples): <b>{check.found} trigger{check.found === 1 ? '' : 's'}</b> in {check.ms.toFixed(0)} ms</p>
            {#if check.events.length}
              <ul class="found">
                {#each check.events as ev, k (k)}<li><span class="muted">{fmtTime(ev.t - check.events[0].t)}</span> {ev.label ?? ''}</li>{/each}
                {#if check.found > check.events.length}<li class="muted">…and {check.found - check.events.length} more</li>{/if}
              </ul>
            {/if}
          {:else}
            <p class="muted">No samples to try it on: start the stream (or the demo) for a dry run.</p>
          {/if}
        {/if}
        {#if check.error}
          <pre class="err">{check.error}</pre>
          {#if badLine}<button class="ghost small" onclick={() => gotoLine(badLine!)}>Go to line {badLine}</button>{/if}
        {/if}
        {#if check.out}
          <details open><summary>Printed</summary><pre>{check.out}</pre></details>
        {/if}
      {:else}
        <p class="muted">Check loads the script and tries it on the latest samples (up to a second), without touching the running trigger. Apply makes the trigger use it.</p>
      {/if}
    </div>

    <footer>
      <span class="muted pos">Ln {caret.line}, Col {caret.col} · Tab indents · Ctrl + / comments · Ctrl + S saves · Ctrl + Enter applies</span>
      {#if note}<span class="note">{note}</span>{/if}
      <span class="grow"></span>
      <button class="ghost" onclick={runCheck} disabled={checking}>{checking ? 'Checking…' : 'Check'}</button>
      <button class="ghost" onclick={() => apply(false)} disabled={applying || applied} title="Ctrl + Enter">Apply</button>
      <button class="primary" onclick={() => apply(true)} disabled={applying} title="Ctrl + Shift + Enter">{applied ? 'Close' : 'Apply and close'}</button>
    </footer>
  </div>
</div>

<style>
  .sbackdrop {
    position: fixed;
    inset: 0;
    background: #0009;
    display: grid;
    place-items: center;
    z-index: 70;
  }
  .sdialog {
    width: min(1100px, 96vw);
    height: min(860px, 94vh);
    display: flex;
    flex-direction: column;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: var(--r-lg);
    padding: 12px 16px 14px;
    gap: 8px;
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  h2 {
    margin: 0;
    font-size: 16px;
  }
  h2 span {
    font-weight: 400;
    font-size: 13px;
  }
  .bar,
  footer {
    display: flex;
    gap: 6px;
    align-items: center;
    flex-wrap: wrap;
  }
  .naming input {
    width: 280px;
    margin-left: 6px;
  }
  .grow {
    flex: 1;
  }
  .sep {
    width: 1px;
    height: 18px;
    background: var(--border);
    margin: 0 4px;
  }
  button.on {
    color: var(--accent);
    border-color: var(--accent);
  }
  .drop {
    position: relative;
  }
  .menu {
    position: absolute;
    top: calc(100% + 4px);
    left: 0;
    z-index: 2;
    width: 380px;
    max-height: 60vh;
    overflow: auto;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: var(--r);
    box-shadow: 0 8px 24px #0006;
    padding: 4px;
  }
  .menu button[role='menuitem'] {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 2px;
    width: 100%;
    text-align: left;
    background: none;
    border: 0;
    padding: 6px 8px;
    border-radius: var(--r);
    color: inherit;
    cursor: pointer;
  }
  .menu button[role='menuitem']:hover {
    background: var(--hover, #ffffff10);
  }
  .menu small {
    color: var(--muted);
    font-size: 11px;
  }
  .saved {
    display: flex;
    align-items: center;
  }
  .del {
    flex: none;
  }
  .empty {
    margin: 6px 8px;
    font-size: 12px;
  }
  .body {
    flex: 1;
    min-height: 0;
    display: flex;
    gap: 8px;
  }
  .editor {
    flex: 1;
    min-width: 0;
    display: flex;
    border: 1px solid var(--border);
    border-radius: var(--r);
    background: var(--scope-bg, var(--bg));
    overflow: hidden;
    font: 13px/1.5 ui-monospace, 'SFMono-Regular', Menlo, Consolas, monospace;
  }
  .editor:focus-within {
    border-color: var(--accent);
  }
  .gutter {
    flex: none;
    padding: 8px 8px 8px 10px;
    text-align: right;
    color: var(--muted);
    opacity: 0.7;
    overflow: hidden;
    user-select: none;
    min-width: 3ch;
    border-right: 1px solid var(--border);
  }
  .gutter .cur {
    color: var(--fg, inherit);
    opacity: 1;
  }
  .gutter .err {
    color: var(--danger);
    opacity: 1;
    font-weight: 700;
  }
  textarea {
    flex: 1;
    min-width: 0;
    border: 0;
    outline: 0;
    resize: none;
    padding: 8px 10px;
    background: transparent;
    color: inherit;
    font: inherit;
    tab-size: 4;
    white-space: pre;
    overflow: auto;
  }
  .ref {
    width: 300px;
    flex: none;
    overflow: auto;
    font-size: 12px;
    border: 1px solid var(--border);
    border-radius: var(--r);
    padding: 4px 10px;
  }
  .ref h3 {
    font-size: 12px;
    margin: 10px 0 4px;
  }
  .ref p,
  .ref ul {
    margin: 0 0 4px;
  }
  .ref ul {
    padding-left: 16px;
  }
  code {
    font-size: 11px;
  }
  .results {
    max-height: 30%;
    overflow: auto;
    font-size: 12px;
    border-top: 1px solid var(--border);
    padding-top: 6px;
  }
  .results p {
    margin: 2px 0;
  }
  .ok {
    color: var(--ok, #6fdc8c);
  }
  .bad {
    color: var(--danger);
  }
  .found {
    margin: 2px 0 4px;
    padding-left: 18px;
    columns: 2;
    font-family: ui-monospace, monospace;
  }
  pre {
    margin: 4px 0;
    max-height: 10em;
    overflow: auto;
    font: 11px/1.4 ui-monospace, monospace;
    padding: 6px;
    border-radius: var(--r);
    background: var(--scope-bg, var(--bg));
    white-space: pre-wrap;
  }
  pre.err {
    color: var(--danger);
    border: 1px solid var(--danger);
  }
  .pos {
    font-size: 11px;
  }
  .note {
    font-size: 12px;
    color: var(--accent);
  }
</style>
