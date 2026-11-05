<script lang="ts">
  // The progress of a software update (Updater.svelte), in a dialog of its
  // own over everything while it runs: the steps (erase, write, read back,
  // the board's own: the gateware test / the firmware check), one bar for
  // the whole, KB, speed and time left; then the outcome. It can't be closed
  // while the update runs, only cancelled (between commands: the active
  // firmware and gateware stay as they are until the last step).

  let {
    title,
    kind,
    stage,
    done,
    total,
    started,
    outcome,
    cancelling,
    oncancel,
    onclose,
  }: {
    title: string;
    kind: 'firmware' | 'gateware';
    /** updater.ts's stage: Erasing, Writing, Verifying, the board's step, Done */
    stage: string;
    done: number;
    total: number;
    /** performance.now() at the start */
    started: number;
    /** null while it runs */
    outcome: { ok: boolean; msg: string; note?: string } | null;
    cancelling: boolean;
    oncancel: () => void;
    onclose: () => void;
  } = $props();

  // weights: the time each step takes over USB (a 440 KB image: erase
  // ~2.6 s, send ~2.3 s, read back ~2.3 s, the board's step 0.3..2 s); the
  // board's own popup uses the same split
  const STEPS = $derived([
    { key: 'Erasing', label: 'Erase the update slot', w: 33 },
    { key: 'Writing', label: 'Send the image', w: 30 },
    { key: 'Verifying', label: 'Read it back', w: 30 },
    { key: 'board', label: kind === 'gateware' ? 'Test it on the FPGA, make it active' : 'Check it; the board restarts to install', w: 7 },
  ]);

  const index = $derived.by(() => {
    if (outcome?.ok || stage === 'Done') return STEPS.length;
    const k = STEPS.findIndex((s) => s.key === stage);
    return k >= 0 ? k : 3; // the board's step has its own text
  });

  const frac = $derived(total ? Math.min(1, done / total) : 0);
  const overall = $derived.by(() => {
    if (outcome?.ok || stage === 'Done') return 100;
    let p = 0;
    for (let k = 0; k < index && k < STEPS.length; k++) p += STEPS[k].w;
    if (index < 3) p += STEPS[index].w * frac;
    return Math.round(p);
  });

  // speed and time left, from the bytes sent and read back so far
  let now = $state(performance.now());
  $effect(() => {
    if (outcome) return;
    const t = setInterval(() => (now = performance.now()), 500);
    return () => clearInterval(t);
  });
  const elapsed = $derived((now - started) / 1000);
  const left = $derived.by(() => {
    if (outcome || overall < 3 || overall >= 100 || elapsed < 2) return '';
    const s = (elapsed * (100 - overall)) / overall;
    return s >= 60 ? `about ${Math.round(s / 60)} min left` : `about ${Math.max(1, Math.round(s))} s left`;
  });
  const kb = (n: number) => `${(n / 1024).toFixed(0)} KB`;
  const detail = $derived.by(() => {
    if (index >= 3 || !total) return '';
    const rate = elapsed > 0.5 && (stage === 'Writing' || stage === 'Verifying') ? ` · ${kb(done / ((now - stepStart) / 1000 || 1))}/s` : '';
    return `${kb(done)} of ${kb(total)}${rate}`;
  });

  // when the current step began, for its speed
  let stepStart = $state(performance.now());
  let lastStage = '';
  $effect(() => {
    if (stage !== lastStage) {
      lastStage = stage;
      stepStart = performance.now();
    }
  });
</script>

<div class="backdrop" role="presentation">
  <div class="dialog" role="alertdialog" aria-label={title} aria-busy={!outcome}>
    <h2>{title}</h2>

    <ol>
      {#each STEPS as s, k (s.key)}
        <li class:done={k < index || (outcome?.ok ?? false)} class:now={k === index && !outcome} class:failed={k === index && outcome && !outcome.ok}>
          <span class="mark">{k < index || outcome?.ok ? '✓' : k === index && outcome ? '✕' : k === index ? '●' : '○'}</span>
          <span>{k === 3 && index === 3 && !outcome && stage !== 'board' ? stage : s.label}</span>
        </li>
      {/each}
    </ol>

    <div class="bar" class:failed={outcome && !outcome.ok}><div style="width: {overall}%"></div></div>
    <div class="line">
      <span>{overall}%</span>
      <span class="muted">{detail}</span>
      <span class="muted right">{left}</span>
    </div>

    {#if outcome}
      <p class={outcome.ok ? 'ok' : 'error'}>{outcome.ok ? '✓' : '✕'} {outcome.msg}</p>
      {#if outcome.note}<p class="muted small">{outcome.note}</p>{/if}
      <div class="buttons"><button class="primary" onclick={onclose}>Close</button></div>
    {:else}
      <p class="muted small">Keep the board connected and powered. Cancelling stops between steps; what runs now stays as it is.</p>
      <div class="buttons">
        <button class="ghost" onclick={oncancel} disabled={cancelling || index >= 3}>{cancelling ? 'Cancelling…' : 'Cancel'}</button>
      </div>
    {/if}
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: #000a;
    display: grid;
    place-items: center;
    z-index: 60;
  }
  .dialog {
    width: min(460px, 92vw);
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: var(--r-lg);
    padding: 16px 20px;
    display: flex;
    flex-direction: column;
    gap: 12px;
    box-shadow: 0 10px 40px #0008;
  }
  h2 {
    margin: 0;
    font-size: 16px;
  }
  ol {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
    font-size: 13px;
  }
  li {
    display: flex;
    gap: 8px;
    color: var(--muted);
  }
  li.done {
    color: var(--ok);
  }
  li.now {
    color: inherit;
    font-weight: 600;
  }
  li.failed {
    color: var(--danger);
  }
  .mark {
    width: 1.2em;
    text-align: center;
  }
  .bar {
    height: 10px;
    border-radius: var(--r-sm);
    background: var(--border);
    overflow: hidden;
  }
  .bar div {
    height: 100%;
    background: var(--accent);
    transition: width 0.25s;
  }
  .bar.failed div {
    background: var(--danger);
  }
  .line {
    display: flex;
    gap: 10px;
    font-size: 13px;
    font-variant-numeric: tabular-nums;
  }
  .right {
    margin-left: auto;
  }
  .buttons {
    display: flex;
    justify-content: flex-end;
  }
  p {
    margin: 0;
  }
  .small {
    font-size: 12px;
  }
  .ok {
    color: var(--ok);
  }
  .error {
    color: var(--danger);
  }
  .muted {
    color: var(--muted);
  }
</style>
