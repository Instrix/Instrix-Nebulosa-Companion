<script lang="ts">
  // The AFE bring-up inside the Debug tab: Playbook (step through a range
  // at a time), Bench (instruments, taps, measure, sequences), Results.
  // Procedure and limits: Instrix-Nebulosa Design/afe1-x10-bringup.md.
  import { onMount } from 'svelte';
  import { isNative } from '../device';
  import * as native from '../native';
  import { hex } from '../nbp';
  import * as afe from './afe';
  import { PLAYBOOKS, operatorStep, type Bringup, type Step, type StepState } from './engine.svelte';
  import { listPorts } from './instruments';

  let { b, connected, streaming, onnote }: { b: Bringup; connected: boolean; streaming: boolean; onnote: (m: string) => void } = $props();

  let tab = $state<'playbook' | 'bench' | 'results'>('playbook');
  let ports = $state<{ path: string; label: string }[]>([]);
  onMount(() => {
    void listPorts().then((p) => (ports = p)).catch(() => undefined);
  });

  const MARK: Record<StepState, string> = { pending: '·', running: '…', pass: '✓', fail: '✗', done: '✓', skipped: '–' };
  const KIND: Record<Step['kind'], string> = {
    check: 'automatic: check',
    timing: 'automatic: switch timing',
    alloff: 'automatic: all off',
    tap: 'automatic: set tap',
    psu: 'automatic: set supply',
    load: 'automatic: set load',
    zero: 'automatic: zero',
    meas: 'automatic: measure',
    info: 'operator',
    box: 'operator: decade box',
    dmm: 'operator: DMM',
  };

  const n = $derived(b.steps.length);
  const cur = $derived(Math.min(b.cur, n));
  const step = $derived(cur < n ? b.steps[cur] : null);
  const pass = $derived(b.steps.filter((s) => s.state === 'pass').length);
  const fail = $derived(b.steps.filter((s) => s.state === 'fail').length);

  // the operator dialog: opens when an operator step comes up (once: Later / Done don't bring it back)
  let dismissed = $state(-1);
  let entry = $state(0);
  let entryFor = -1;
  const dialogOpen = $derived(!!step && operatorStep(step.kind) && step.state === 'pending' && !b.busy && !b.executing && dismissed !== cur);
  $effect(() => {
    if (dialogOpen && entryFor !== cur && step) {
      entryFor = cur;
      entry = step.kind === 'box' ? step.boxOhm : 0;
    }
  });
  /** the measurement just before the dialog, so the dialog doesn't hide it */
  const before = $derived.by(() => {
    for (let j = cur - 1; j >= 0 && j >= cur - 4; j--) if (b.steps[j].kind === 'meas' || b.steps[j].kind === 'zero') return b.steps[j];
    return null;
  });
  function done() {
    dismissed = cur;
    void b.pbOperatorDone(cur, Number(entry));
  }
  function reopen() {
    if (step) step.state = 'pending';
    entryFor = -1;
    dismissed = -1;
  }
  function jump(i: number) {
    if (b.busy || b.executing) return;
    b.cur = i;
    entryFor = -1;
  }
  function stopAll() {
    if (step) b.steps[cur].state = 'fail';
    if (step) b.steps[cur].result = 'stopped: ALL OFF';
    dismissed = cur;
    void b.allOff('ALL OFF (playbook)');
  }

  const val = (x: number, volts: boolean) => (Number.isNaN(x) ? '—' : volts ? `${x.toFixed(3)} V` : afe.siAmps(x));
  const fmt = (x: number, d: number, unit = '') => (Number.isNaN(x) ? '—' : `${x.toFixed(d)}${unit}`);

  async function saveCsv() {
    const bytes = new TextEncoder().encode(b.csv());
    const name = 'afe_bringup.csv';
    if (isNative) {
      const path = await native.pickSave(name, 'CSV', 'csv');
      if (!path) return;
      await native.saveBytes(path, bytes);
      onnote(`Saved ${b.rows.length} rows to ${path}`);
    } else {
      const a = document.createElement('a');
      a.href = URL.createObjectURL(new Blob([bytes as BlobPart], { type: 'text/csv' }));
      a.download = name;
      a.click();
      setTimeout(() => URL.revokeObjectURL(a.href), 2000);
    }
  }

  const dec = $derived(b.rangeDec);
  const fits = $derived(b.overrideIl || (b.manualRange >= 0 && b.loadCcMa * 1e-3 <= afe.ilLimit(b.manualRange) + 1e-9));
  const COMP = ['UA', 'DA', 'UB', 'DB', 'UC', 'DC'];
</script>

<div class="bu">
  <div class="top">
    <button class="alloff" onclick={() => b.allOff()}>ALL OFF: load input + CH1</button>
    <p class="muted small">
      Procedure: Instrix-Nebulosa <code>Design/afe1-x10-bringup.md</code>. Never put current through I+ / I− unless a tap is on in manual mode; CH1’s current limit is the last line of defence.
    </p>
  </div>

  <div class="tabs">
    <button class:on={tab === 'playbook'} onclick={() => (tab = 'playbook')}>Playbook</button>
    <button class:on={tab === 'bench'} onclick={() => (tab = 'bench')}>Bench</button>
    <button class:on={tab === 'results'} onclick={() => (tab = 'results')}>Results <small>{b.rows.length}</small></button>
    <span class="sp"></span>
    <span class="small muted">status: {b.status}</span>
  </div>

  <!-- the Nebulosa, briefly: on every page -->
  <div class="neb small">
    {#if !connected}
      <span class="bad">Nebulosa not connected</span>
    {:else if !streaming}
      <span class="warn">stream stopped</span>
    {:else}
      <span class="ok">streaming</span>
    {/if}
    {#if dec}
      · {#if dec.automatic}<span class="warn">AUTOMATIC ranging: take a tap before any current</span>{:else}manual: tap {dec.manual + 1}, FS {afe.FS_LABEL[dec.manual]}, R {+afe.R_CUM[dec.manual].toPrecision(3)} Ω{/if}
      · EN_TAP {#each afe.FS_A as _, i (i)}<span class="pill" class:on={(dec.taps >> i) & 1}>{i + 1}</span>{/each}
      · pins {#each COMP as c, i (i)}<span class="pill warnpill" class:on={(dec.pins >> i) & 1}>{c}</span>{/each}
    {/if}
    · PSU {b.psuOpen ? (b.psu.simulated ? 'sim' : 'open') : '—'}{b.psuOut[0] ? ', CH1 ON' : ''} · load {b.loadOpen ? (b.load.simulated ? 'sim' : 'open') : '—'}{b.loadIn ? `, ON ${afe.siAmps(b.loadSet)}` : ''}
  </div>

  {#if tab === 'playbook'}
    <div class="row wrap">
      <select bind:value={b.pbKind} disabled={b.busy}>
        {#each PLAYBOOKS as p, i (i)}<option value={i}>{p}</option>{/each}
      </select>
      <label>V box <input type="number" min="2" max="30" step="0.5" bind:value={b.pbBoxV} disabled={b.busy} /></label>
      <label title="the decade box's tolerance">box tol % <input type="number" min="0" step="0.1" bind:value={b.pbBoxTol} disabled={b.busy} /></label>
      <label title="uncalibrated gain tolerance, % of the reading">gain tol % <input type="number" min="0" step="0.1" bind:value={b.pbGainTol} disabled={b.busy} /></label>
      <button onclick={() => b.pbBuild()} disabled={b.busy}>Build / restart</button>
      <label class="check"><input type="checkbox" bind:checked={b.pbAuto} /> auto-run automatic steps</label>
    </div>

    {#if !n}
      <p class="hint">
        Pick a playbook and press Build. Each tap: the tap is set, its zero taken with the loop open, then 10 / 50 / 85 % of full scale from the load (taps 1-2) or from CH1 through the decade box (taps 3-6), each measured against the reference with a pass / fail limit.
        Automatic steps (tap, supply, load, measure) run by themselves; operator steps (wiring, box, DMM) open a dialog. Open the instruments on the Bench first (<code>sim</code> for simulated ones: with the demo device, the simulated loop current drives its ADC).
      </p>
    {:else}
      <div class="prog small">{b.pbName}: step {Math.min(cur + 1, n)} of {n} · <span class="ok">pass {pass}</span> · <span class:bad={fail > 0}>fail {fail}</span></div>
      <progress value={cur} max={n}></progress>
      <div class="pb">
        <ol class="list">
          {#each b.steps as s, i (i)}
            <li class:cur={i === cur}>
              <button class="item" onclick={() => jump(i)} disabled={b.busy || b.executing}>
                <span class="mark {s.state}">{MARK[s.state]}</span>
                <span class="t">{s.title}</span>
              </button>
            </li>
          {/each}
        </ol>
        <div class="stepbox">
          {#if !step}
            <p class:bad={fail > 0} class:ok={!fail}><strong>Playbook finished: {pass} passed, {fail} failed.</strong></p>
            <p class="small">The rows are in Results: save the CSV, and fill the results table of afe1-x10-bringup.md.</p>
          {:else}
            <p class="muted small">step {cur + 1}, {KIND[step.kind]}</p>
            <h3>{step.title}</h3>
            {#if step.text}<p class="small">{step.text}</p>{/if}
            {#if step.kind === 'meas' || step.kind === 'zero' || step.kind === 'load' || step.kind === 'psu'}
              {@const volts = step.kind === 'psu'}
              <table class="vals">
                <thead><tr><th>expected</th><th>read</th><th>error, % FS</th><th>limit, % FS</th></tr></thead>
                <tbody>
                  <tr>
                    <td>{val(step.kind === 'meas' && Number.isNaN(step.expect) ? step.iSet : step.expect, volts)}</td>
                    <td>{val(step.read, volts)}</td>
                    <td class={step.state}>{Number.isNaN(step.errFs) ? '—' : (step.errFs >= 0 ? '+' : '') + step.errFs.toFixed(3)}</td>
                    <td>{Number.isNaN(step.tol) ? '—' : `±${step.tol.toFixed(3)}`}</td>
                  </tr>
                </tbody>
              </table>
            {/if}
            {#if step.result}<p class="result {step.state}">{MARK[step.state]} {step.result}</p>{/if}
            <div class="row">
              {#if !operatorStep(step.kind)}
                <button class="primary" onclick={() => b.pbExec(cur)} disabled={b.busy || b.executing}>{step.state === 'pending' ? 'Run step' : 'Run again'}</button>
              {:else}
                <button class="primary" onclick={reopen} disabled={b.busy || b.executing}>Open dialog</button>
              {/if}
              <button onclick={() => b.skip(cur)} disabled={b.busy || b.executing}>Skip</button>
              <button onclick={() => jump(Math.max(0, cur - 1))} disabled={b.busy || b.executing || cur === 0}>Back</button>
              {#if b.executing}<button class="ghost" onclick={() => b.abortNow()}>Stop</button>{/if}
            </div>
            {#if !connected}<p class="bad small">Nebulosa not connected</p>{/if}
          {/if}
        </div>
      </div>
    {/if}
  {:else if tab === 'bench'}
    <div class="grid">
      <section>
        <h4>Instruments</h4>
        <datalist id="inst-ports">
          <option value="sim">simulated</option>
          {#each ports as p (p.path)}<option value={p.path}>{p.label}</option>{/each}
        </datalist>
        <div class="row wrap">
          <input class="path" list="inst-ports" bind:value={b.psuPath} disabled={b.busy} />
          <button onclick={() => b.openPsu()} disabled={b.busy}>Open SPD3303C</button>
        </div>
        <div class="row wrap">
          <input class="path" list="inst-ports" bind:value={b.loadPath} disabled={b.busy} />
          <button onclick={() => b.openLoad()} disabled={b.busy}>Open KEL102</button>
          <select bind:value={b.loadBaud} disabled={b.busy}>{#each [9600, 19200, 38400, 57600, 115200] as r (r)}<option value={r}>{r}</option>{/each}</select>
        </div>
        <p class="muted small">PSU: {b.psuOpen ? b.psuIdn : 'not open'}<br />load: {b.loadOpen ? b.loadIdn : 'not open'}</p>
        <p class="muted small">
          {#if isNative}<code>/dev/usbtmcN</code> (USBTMC), a serial port (<code>/dev/ttyACMn</code>, <code>COMn</code>), <code>tcp:HOST[:PORT]</code>, or <code>sim</code>.{:else}The browser build has the simulated instruments only (<code>sim</code>); the desktop app opens real ones.{/if}
        </p>
        {#if b.psuOpen || b.loadOpen}<button class="ghost small" onclick={() => b.closeInstruments()} disabled={b.busy}>Close both (after ALL OFF)</button>{/if}
      </section>

      <section>
        <h4>SPD3303C</h4>
        {#each [1, 2] as ch (ch)}
          <p class="small">CH{ch} {ch === 1 ? '(test current: + to I+)' : '(Nebulosa 12 V barrel)'}</p>
          <div class="row wrap">
            <label><input type="number" min="0" max="32" step="0.1" bind:value={b.ch[ch - 1].v} disabled={!b.psuOpen || b.busy} /> V</label>
            <label><input type="number" min="0" max="3.2" step="0.01" bind:value={b.ch[ch - 1].a} disabled={!b.psuOpen || b.busy} /> A limit</label>
            <button onclick={() => b.setPsu(ch)} disabled={!b.psuOpen || b.busy}>Set</button>
            <button class:onbtn={b.psuOut[ch - 1]} onclick={() => b.togglePsu(ch)} disabled={!b.psuOpen || b.busy}>{b.psuOut[ch - 1] ? 'Output OFF' : 'Output ON'}</button>
            <span class="mono">{fmt(b.psuV[ch - 1], 3, ' V')} {fmt(b.psuI[ch - 1], 3, ' A')}</span>
          </div>
        {/each}
        <p class="muted small">Readback 10 mV / 10 mA, ±(0.5 % + 2 digits): not a reference.</p>
      </section>

      <section>
        <h4>KEL102</h4>
        <div class="row wrap">
          <label><input type="number" min="0" max="3000" step="0.1" bind:value={b.loadCcMa} disabled={!b.loadOpen || b.busy} /> mA CC</label>
          <button onclick={() => b.loadCc(b.loadCcMa * 1e-3, b.loadIn)} disabled={!b.loadOpen || b.busy || !fits}>Apply CC</button>
          <button class:onbtn={b.loadIn} onclick={() => b.toggleLoad()} disabled={!b.loadOpen || b.busy || (!b.loadIn && !fits)}>{b.loadIn ? 'Input OFF' : 'Input ON'}</button>
          <span class="mono">{fmt(b.loadV, 4, ' V')} {fmt(b.loadI * 1e3, 2, ' mA')}</span>
        </div>
        {#if b.loadOpen && !fits}
          <p class="bad small">
            {#if b.manualRange < 0}interlock: no manual tap (automatic ranging): take a tap first{:else}interlock: {afe.siAmps(b.loadCcMa * 1e-3)} &gt; 1.1 × FS of tap {b.manualRange + 1} ({afe.siAmps(afe.ilLimit(b.manualRange))}){/if}
          </p>
        {/if}
        <p class="muted small">CC 0-3 A: 0.1 mA steps, ±(0.05 % + 1.5 mA): a reference on taps 1-2 only.</p>
      </section>

      <section>
        <h4>Nebulosa</h4>
        <div class="row wrap">
          <span class="lbl">go to tap</span>
          {#each afe.FS_LABEL as fs, i (i)}<button class="tap" class:onbtn={b.manualRange === i} title={fs} onclick={() => b.goTap(i)} disabled={!connected || b.busy}>{i + 1}</button>{/each}
          <span class="muted small">(down one tap at a time)</span>
        </div>
        <div class="row wrap">
          <button onclick={() => b.setOverlap().then((m) => onnote(`Range timing: ${m}`), (e) => onnote(String(e)))} disabled={!connected || b.busy}>Bring-up switch timing: 0.5 µs overlap</button>
          {#if dec}<span class="muted small">now {dec.overlap + 1} cycles = {(((dec.overlap + 1) / b.clkHz) * 1e6).toFixed(2)} µs</span>{/if}
        </div>
        <label class="small">chain map
          <select bind:value={b.chainMap}>
            <option value="x25">gateware ×25 map: 0/1 A, 2/3 B, 4/5 C</option>
            <option value="x10">TR-0002 ×10 map: 0-2 A, 3-4 B, 5 C</option>
          </select>
        </label>
        {#if dec}<p class="muted small mono">RANGE_STATUS taps {hex(dec.taps, 2)} pins {hex(dec.pins, 2)}</p>{/if}
      </section>

      <section>
        <h4>Measure</h4>
        <div class="row wrap">
          <label class="check"><input type="radio" bind:group={b.refKind} value="load" /> ref: KEL102 current</label>
          <label class="check"><input type="radio" bind:group={b.refKind} value="box" /> V / decade box</label>
          <label class="check"><input type="radio" bind:group={b.refKind} value="typed" /> typed</label>
        </div>
        {#if b.refKind === 'box'}
          <div class="row wrap">
            <label><input type="number" min="0" bind:value={b.boxOhm} /> Ω box</label>
            <label><input type="number" min="0" step="0.001" bind:value={b.boxV} /> V across the loop</label>
            <span class="muted small">I = {afe.siAmps(b.boxOhm > 0 ? b.boxV / b.boxOhm : NaN)}</span>
          </div>
        {:else if b.refKind === 'typed'}
          <div class="row"><label><input type="number" step="0.001" bind:value={b.typedUa} /> µA</label></div>
        {/if}
        <div class="row wrap">
          <label><input type="number" min="20" max="2000" step="100" bind:value={b.measMs} /> ms</label>
          <button class="primary" onclick={() => b.measureNow()} disabled={b.busy || b.manualRange < 0}>Measure</button>
          <label class="check"><input type="checkbox" bind:checked={b.overrideIl} /> override interlock</label>
          {#if b.overrideIl}<span class="bad small">OFF: over-range pulses only (§9)</span>{/if}
        </div>
        {#if b.last?.ok}
          {@const m = b.last}
          {@const c = m.mean - b.zeroOf(m.range)}
          {@const a = afe.codeToAmps(c, m.range, b.chainMap)}
          <p class="small mono">tap {m.range + 1}: {m.mean.toFixed(2)} codes (zero {b.zeroOf(m.range).toFixed(2)}), {m.sd.toFixed(2)} rms over {m.n} samples ({m.dirty} dirty): {afe.siAmps(a)}, {((a / afe.FS_A[m.range]) * 100).toFixed(3)} % FS</p>
        {/if}
      </section>

      <section>
        <h4>Sequences</h4>
        <div class="row wrap">
          <button onclick={() => b.runSequence('zero', () => b.seqZero())} disabled={b.busy}>Zero + noise, taps 1-6 (§5)</button>
          <span class="muted small">loop open: load off, CH1 off</span>
        </div>
        <div class="row wrap">
          <select bind:value={b.linTap} disabled={b.busy}>{#each [1, 2, 3] as t (t)}<option value={t}>tap {t}</option>{/each}</select>
          <label><input type="number" min="2" max="12" step="0.5" bind:value={b.linV} /> V CH1</label>
          <button onclick={() => b.runSequence('linearity', () => b.seqLinearity(b.linTap, Math.min(12, Math.max(2, b.linV))))} disabled={b.busy || !b.psuOpen || !b.loadOpen}>Linearity, KEL102 CC (§6)</button>
        </div>
        <div class="row wrap">
          <label><input type="number" min="2" max="30" step="0.5" bind:value={b.ratioV} /> V box</label>
          <button onclick={() => b.runSequence('ratio', () => b.seqRatio(Math.min(30, Math.max(2, b.ratioV))))} disabled={b.busy || !b.psuOpen || !b.loadOpen}>Ladder ratio transfer 1→6 (§7)</button>
        </div>
        <div class="row wrap">
          <select bind:value={b.cmpTap} disabled={b.busy}>{#each [1, 2, 3] as t (t)}<option value={t}>tap {t}</option>{/each}</select>
          <button onclick={() => b.runSequence('comparators', () => b.seqComparators(b.cmpTap))} disabled={b.busy || !b.psuOpen || !b.loadOpen}>Comparator trip points (§8)</button>
        </div>
        {#if !b.psuOpen || !b.loadOpen}<p class="muted small">Open both instruments for the load sequences.</p>{/if}
        <div class="row">
          <span class="small">status: {b.status}</span>
          {#if b.busy && !b.executing}<button class="ghost small" onclick={() => b.abortNow()}>Abort</button>{/if}
        </div>
        {#if b.prompt}
          <div class="prompt">
            <p>{b.prompt.text}</p>
            <div class="row"><button class="primary" onclick={() => b.promptDone(true)}>Continue</button><button onclick={() => b.promptDone(false)}>Abort</button></div>
          </div>
        {/if}
      </section>
    </div>
  {:else}
    <div class="row">
      <button onclick={saveCsv} disabled={!b.rows.length}>Save CSV…</button>
      <button class="ghost" onclick={() => (b.rows = [])} disabled={!b.rows.length}>Clear</button>
      <button class="ghost" onclick={() => b.forgetZeros()}>Forget zeros</button>
      <span class="muted small">zeros: {b.zero.map((z) => (Number.isNaN(z) ? '—' : z.toFixed(1))).join(' / ')}</span>
    </div>
    <div class="results">
      <table>
        <thead><tr><th>test</th><th>tap</th><th>reference</th><th>reading</th><th>err / value %</th><th>rms codes</th><th>dirty</th><th>note</th></tr></thead>
        <tbody>
          {#each b.rows as w, i (i)}
            <tr>
              <td>{w.test}</td>
              <td>{w.tap}</td>
              <td class="mono">{afe.siAmps(w.iRef)}</td>
              <td class="mono">{afe.siAmps(w.iNeb)}</td>
              <td class="mono">{Number.isNaN(w.err) ? '—' : (w.err >= 0 ? '+' : '') + w.err.toFixed(4)}</td>
              <td class="mono">{fmt(w.sd, 2)}</td>
              <td>{(w.dirty * 100).toFixed(0)}%</td>
              <td class="note">{w.note}</td>
            </tr>
          {/each}
          {#if !b.rows.length}<tr><td colspan="8" class="muted">No measurements yet.</td></tr>{/if}
        </tbody>
      </table>
    </div>
  {/if}
</div>

{#if dialogOpen && step}
  <div class="modal-bg" role="presentation">
    <div class="modal" role="dialog" aria-modal="true" aria-label="Operator step">
      <p class="muted small">step {cur + 1} of {n}</p>
      {#if before}<p class="result small {before.state}">{MARK[before.state]} {before.title}: {before.result}</p>{/if}
      <h3>{step.title}</h3>
      <p>{step.text}</p>
      {#if step.kind === 'box' || step.kind === 'dmm'}
        <!-- svelte-ignore a11y_autofocus -->
        <label class="entry">{step.kind === 'box' ? 'Ω dialled' : 'Ω read'} <input type="number" step="any" bind:value={entry} autofocus onkeydown={(e) => e.key === 'Enter' && done()} /></label>
        <p class="muted small">{step.kind === 'dmm' ? `expected ${step.lo} .. ${step.hi} ${step.unit}` : `planned ${afe.ohms(step.boxOhm)}; the measurement uses what you enter`}</p>
      {/if}
      <div class="row">
        <button class="primary" onclick={done}>{step.kind === 'info' ? 'Done, continue' : 'Done'}</button>
        <button onclick={() => (dismissed = cur)}>Later</button>
        <span class="sp"></span>
        <button class="alloff small" onclick={stopAll}>ALL OFF, stop</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .bu {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .top {
    display: flex;
    align-items: center;
    gap: 12px;
  }
  .top p {
    margin: 0;
  }
  .alloff {
    background: #b3261e;
    color: #fff;
    border-color: transparent;
    font-weight: 700;
    padding: 8px 14px;
    white-space: nowrap;
  }
  .alloff:hover:not(:disabled) {
    background: #d0342a;
  }
  .alloff.small {
    padding: 4px 10px;
  }
  .tabs {
    display: flex;
    gap: 4px;
    align-items: center;
    border-bottom: 1px solid var(--border);
  }
  .tabs button {
    border: none;
    border-bottom: 2px solid transparent;
    border-radius: 0;
    background: none;
  }
  .tabs button.on {
    border-bottom-color: var(--accent);
    color: var(--accent);
  }
  .sp {
    flex: 1;
  }
  .neb {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    align-items: center;
  }
  .pill {
    font-size: 10.5px;
    padding: 0 5px;
    border-radius: var(--r-sm);
    border: 1px solid var(--border);
    color: var(--muted);
  }
  .pill.on {
    background: color-mix(in srgb, var(--ok) 25%, transparent);
    border-color: var(--ok);
    color: var(--text);
  }
  .warnpill.on {
    background: color-mix(in srgb, var(--warn) 25%, transparent);
    border-color: var(--warn);
  }
  .small {
    font-size: 11.5px;
  }
  .muted {
    color: var(--muted);
  }
  .mono {
    font-family: ui-monospace, 'SF Mono', Menlo, Consolas, monospace;
    font-size: 12px;
  }
  .ok,
  .pass,
  .done {
    color: var(--ok);
  }
  .bad,
  .fail {
    color: var(--danger);
  }
  .warn {
    color: var(--warn);
  }
  .running {
    color: var(--accent);
  }
  .skipped {
    color: var(--muted);
  }
  .row {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 4px 0;
  }
  .row.wrap {
    flex-wrap: wrap;
  }
  label {
    font-size: 12px;
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }
  input[type='number'] {
    width: 76px;
  }
  select,
  input[type='radio'] {
    width: auto;
  }
  label.check {
    white-space: nowrap;
  }
  input.path {
    width: 170px;
  }
  .hint {
    font-size: 12.5px;
    color: var(--muted);
    max-width: 80ch;
  }
  progress {
    width: 100%;
    height: 4px;
  }
  .pb {
    display: grid;
    grid-template-columns: minmax(240px, 340px) 1fr;
    gap: 10px;
    min-height: 360px;
  }
  .list {
    list-style: none;
    margin: 0;
    padding: 4px;
    border: 1px solid var(--border);
    border-radius: var(--r);
    max-height: 62vh;
    overflow-y: auto;
  }
  .list li.cur .item {
    background: color-mix(in srgb, var(--accent) 16%, transparent);
  }
  .item {
    display: flex;
    gap: 8px;
    width: 100%;
    text-align: left;
    border: none;
    background: none;
    padding: 2px 6px;
    font-size: 12px;
    border-radius: var(--r-sm);
  }
  .mark {
    width: 12px;
    flex: none;
    font-weight: 700;
  }
  .mark.pending {
    color: var(--muted);
  }
  .stepbox {
    border: 1px solid var(--border);
    border-radius: var(--r);
    padding: 10px 14px;
  }
  .stepbox h3 {
    margin: 2px 0 6px;
    font-size: 17px;
  }
  .stepbox p {
    margin: 4px 0;
  }
  .vals {
    border-collapse: collapse;
    margin: 8px 0;
    font-size: 12.5px;
  }
  .vals th,
  .vals td {
    border: 1px solid var(--border);
    padding: 3px 10px;
    text-align: left;
  }
  .vals th {
    color: var(--muted);
    font-weight: 600;
  }
  .result {
    font-size: 12.5px;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(360px, 1fr));
    gap: 10px;
    align-items: start;
  }
  section {
    border: 1px solid var(--border);
    border-radius: var(--r);
    padding: 8px 12px;
    min-width: 0;
  }
  h4 {
    margin: 0 0 6px;
    font-size: 13px;
  }
  section p {
    margin: 4px 0;
  }
  .lbl {
    font-size: 11.5px;
    color: var(--muted);
  }
  .tap {
    min-width: 30px;
    padding: 2px 6px;
  }
  .onbtn {
    background: var(--warn);
    color: #111;
    border-color: transparent;
  }
  .tap.onbtn {
    background: var(--ok);
    color: #fff;
  }
  .prompt {
    border: 1px solid var(--warn);
    background: color-mix(in srgb, var(--warn) 14%, transparent);
    border-radius: var(--r);
    padding: 8px 10px;
    font-size: 12.5px;
  }
  .results {
    overflow: auto;
    max-height: 70vh;
  }
  .results table {
    border-collapse: collapse;
    width: 100%;
    font-size: 12px;
  }
  .results th {
    text-align: left;
    color: var(--muted);
    border-bottom: 1px solid var(--border);
    padding: 3px 8px 3px 0;
    position: sticky;
    top: 0;
    background: var(--panel);
  }
  .results td {
    padding: 2px 8px 2px 0;
    white-space: nowrap;
    vertical-align: top;
  }
  .results td.note {
    white-space: normal;
    min-width: 240px;
  }
  .modal-bg {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.45);
    display: grid;
    place-items: center;
    z-index: 50;
  }
  .modal {
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: var(--r-lg);
    padding: 14px 18px;
    width: min(560px, calc(100vw - 32px));
    box-shadow: 0 10px 40px rgba(0, 0, 0, 0.35);
  }
  .modal h3 {
    margin: 4px 0 8px;
    font-size: 18px;
  }
  .modal p {
    font-size: 13px;
    margin: 6px 0;
  }
  .entry input {
    width: 180px;
    font-size: 15px;
  }
  @media (max-width: 700px) {
    .pb {
      grid-template-columns: 1fr;
    }
    .top {
      flex-direction: column;
      align-items: stretch;
    }
  }
</style>
