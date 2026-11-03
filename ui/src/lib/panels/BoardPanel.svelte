<script lang="ts">
  // The board: its power inputs and USB-PD contract, the analog front end's
  // supplies (off: the ADC reads nothing real), its clock, the self-tests
  // and the rest of its status. Polled every 2 s.
  import { onMount } from 'svelte';
  import * as bd from '../board';
  import type { Device } from '../device';
  import { hex } from '../nbp';
  import { fmtBytes } from '../si';

  let {
    dev,
    streaming,
    board = $bindable(),
    onstop,
    onstart,
    onnote,
  }: {
    dev: Device;
    streaming: boolean;
    /** the latest status (null: none yet, or no board commands) */
    board: bd.BoardStatus | null;
    /** stops the stream (the self-tests and PD changes need it stopped) */
    onstop: () => Promise<void>;
    /** starts the stream again (after the self-tests stopped it) */
    onstart: () => Promise<void>;
    onnote: (msg: string) => void;
  } = $props();

  let error = $state('');
  let busy = $state(false);
  let testing = $state(false);
  let testOk = $state<boolean | null>(null);
  let unsupported = $state(false);
  let showTests = $state(false);

  // the board's clock set to this computer's when it connects (and is off by more than 2 s)
  const AUTO_KEY = 'nebulosa.clockAutoSync';
  let autoSync = $state(loadAuto());
  function loadAuto() {
    try {
      return localStorage.getItem(AUTO_KEY) !== '0';
    } catch {
      return true;
    }
  }
  function setAuto(on: boolean) {
    autoSync = on;
    try {
      localStorage.setItem(AUTO_KEY, on ? '1' : '0');
    } catch {
      /* storage unavailable */
    }
  }
  let firstStatus = true;

  async function poll() {
    if (busy || unsupported) return;
    try {
      const s = await bd.status(dev);
      if (!s) unsupported = true;
      board = s;
      if (s && firstStatus) {
        firstStatus = false;
        const off = s.rtc_s - Date.now() / 1000;
        if (autoSync && (!s.rtc_running || Math.abs(off) > 2)) {
          await act(() => bd.syncClock(dev));
          if (!error) onnote(`Board clock synced to this computer's (it was ${s.rtc_running ? fmtOffset(off) : 'not running'}).`);
        }
      }
    } catch {
      /* a missed poll: the next one */
    }
  }

  onMount(() => {
    poll();
    const t = setInterval(poll, 2000);
    return () => clearInterval(t);
  });

  async function act(what: () => Promise<bd.BoardStatus | null>) {
    error = '';
    busy = true;
    try {
      const s = await what();
      if (s) board = s;
    } catch (e) {
      error = bd.reason(e);
      const s = (e as { board?: bd.BoardStatus | null }).board;
      if (s) board = s;
    } finally {
      busy = false;
    }
  }

  const supplies = $derived(board ? (board.apwr_h ? 1 : 0) + (board.apwr_l ? 1 : 0) : 0);

  function setSupplies(on: boolean) {
    act(async () => {
      await bd.set(dev, bd.SET.APWR_H, on ? 1 : 0);
      return bd.set(dev, bd.SET.APWR_L, on ? 1 : 0);
    });
  }

  /** the charger's fixed voltages */
  const pdos = $derived((board?.pd_pdos ?? []).filter((p) => p.fixed && p.mv > 0));
  const offers12 = $derived(pdos.find((p) => p.mv >= 12000 && p.mv <= 15000));

  async function requestPd(mv: number) {
    if (streaming && !confirm('Changing the USB-C voltage needs the stream stopped. Stop it?')) return;
    if (streaming) await onstop();
    await act(() => bd.set(dev, bd.SET.PD_MV, mv));
    if (!error) onnote(`USB-C: asked the charger for ${(mv / 1000).toFixed(0)} V`);
  }

  /** board clock minus this computer's, s */
  const clockOff = $derived(board?.rtc_running ? board.rtc_s - Date.now() / 1000 : null);

  async function syncClock() {
    await act(() => bd.syncClock(dev));
    if (!error) onnote("The board's clock is set to this computer's.");
  }

  /** the stream stops for the tests (they take the buses) and starts again after */
  async function runTests() {
    const wasStreaming = streaming;
    if (wasStreaming && !confirm('The self-tests stop the stream for a few seconds (it starts again after). Run them?')) return;
    showTests = true;
    if (wasStreaming) await onstop();
    error = '';
    testing = true;
    busy = true;
    try {
      const r = await bd.selfTest(dev);
      testOk = r.ok;
      if (r.board) board = r.board;
    } catch (e) {
      error = bd.reason(e);
    } finally {
      testing = false;
      busy = false;
    }
    if (wasStreaming) await onstart();
  }

  const ran = $derived(board?.tests.filter((t) => t.result !== 'none') ?? []);
  const failed = $derived(ran.filter((t) => t.result === 'fail').length);

  /** a test's detail in its unit */
  function fmtDetail(t: bd.TestResult) {
    if (!t.detail_unit || (t.result === 'skip' && !t.detail)) return '';
    if (t.detail_unit === 'ID' || t.detail_unit === 'PHY ID') return hex(t.detail, 6);
    if (t.detail_unit.endsWith('Hz')) return t.detail >= 1e6 ? `${(t.detail / 1e6).toFixed(2)} MHz` : `${t.detail.toLocaleString()} Hz`;
    if (t.detail_unit.endsWith('mV')) return `${(t.detail / 1000).toFixed(2)} V`;
    if (t.detail_unit === 'error step') return t.result === 'fail' ? `failed at step ${t.detail}` : '';
    return `${t.detail} ${t.detail_unit}`;
  }

  const v = (mv: number) => (mv < 500 ? '–' : `${(mv / 1000).toFixed(2)} V`);
  const MARK = { pass: '✓', fail: '✕', skip: '–', none: '·' } as const;

  function fmtUptime(ms: number) {
    const s = Math.floor(ms / 1000);
    const d = Math.floor(s / 86400), h = Math.floor((s % 86400) / 3600), m = Math.floor((s % 3600) / 60);
    return d ? `${d} d ${h} h` : h ? `${h} h ${m} min` : `${m} min ${s % 60} s`;
  }
  function fmtOffset(s: number) {
    const a = Math.abs(s);
    const t = a < 120 ? `${a.toFixed(0)} s` : a < 7200 ? `${(a / 60).toFixed(0)} min` : a < 172800 ? `${(a / 3600).toFixed(1)} h` : `${(a / 86400).toFixed(0)} days`;
    return `${t} ${s < 0 ? 'behind' : 'ahead'}`;
  }
</script>

{#if !unsupported}
  <section class="board">
    <h2>Board</h2>
    {#if !board}
      <small class="muted">Asking the board…</small>
    {:else}
      <div class="fe" class:on={supplies === 2} class:off={supplies === 0}>
        <div class="fe-head">
          <span class="dot"></span>
          <b title={supplies === 2 ? 'On' : supplies === 1 ? 'One of its two supplies on' : 'Off'}>Analog front end</b>
          <button
            class="small"
            class:primary={supplies < 2}
            class:ghost={supplies === 2}
            disabled={busy || (!board.apwr_ok && supplies === 0)}
            onclick={() => setSupplies(supplies < 2)}
            title="The isolated supplies H and L of the measurement side">{supplies === 2 ? 'Turn off' : 'Turn on'}</button
          >
        </div>
        {#if !board.apwr_ok}
          <p class="small warn">
            Needs 12 V: plug in the 12 V jack{#if board.usb_c && offers12}, or
              <button class="link" disabled={busy} onclick={() => requestPd(offers12!.mv)}>ask the USB-C charger for {offers12.mv / 1000} V</button>{/if}.
          </p>
        {:else if supplies === 0}
          <p class="small muted">Off: the ADC reads nothing real.</p>
        {/if}
      </div>

      <dl>
        <dt>12 V jack</dt><dd>{v(board.v_bj_mv)}</dd>
        <dt>USB-C</dt>
        <dd>
          {v(board.v_vbus_mv)}
          {#if board.usb_c}<small class="muted">{board.pd_state === 4 ? `PD ${board.pd_mv / 1000} V / ${(board.pd_ma / 1000).toFixed(1)} A` : bd.PD_STATE[board.pd_state] ?? ''}</small>{/if}
        </dd>
        <dt>Supply</dt><dd>{v(board.v_main_mv)}</dd>
        <dt>Clock</dt>
        <dd>
          {#if clockOff == null}<span class="warn">not running</span>
          {:else if Math.abs(clockOff) < 3}<span class="muted">in sync</span>
          {:else}<span class="warn">{fmtOffset(clockOff)}</span>{/if}
        </dd>
      </dl>

      <div class="actions">
        <button class="ghost small" disabled={busy} onclick={syncClock} title="Set the board's clock (its RTC) to this computer's">Sync clock</button>
        <button class="ghost small" disabled={busy} onclick={runTests} title="Crystals, clock, supplies, flash, SD card, screen, FPGA, Ethernet, USB PD">
          {testing ? 'Testing…' : 'Self-test'}
        </button>
        {#if ran.length && !testing}
          <button class="link small" class:fail={failed} onclick={() => (showTests = true)}>{failed ? `${failed} failed` : `${ran.filter((t) => t.result === 'pass').length} passed`}</button>
        {/if}
      </div>
      <label class="check small"><input type="checkbox" checked={autoSync} onchange={(e) => setAuto((e.currentTarget as HTMLInputElement).checked)} /> Sync the clock on connect</label>

      <details>
        <summary><h2>Power and board</h2></summary>
        <div class="more">
          {#if board.usb_c && pdos.length}
            <label>
              USB-C voltage
              <select value={board.pd_target_mv} disabled={busy} onchange={(e) => requestPd(Number((e.currentTarget as HTMLSelectElement).value))}>
                {#each pdos as p (p.mv)}<option value={p.mv}>{p.mv / 1000} V · {(p.ma / 1000).toFixed(1)} A max</option>{/each}
              </select>
            </label>
          {/if}
          <label>
            Power input
            <select value={board.pwr_sel} disabled={busy} onchange={(e) => act(() => bd.set(dev, bd.SET.PWR_SEL, Number((e.currentTarget as HTMLSelectElement).value)))}>
              {#each bd.PWR_SEL as name, k (k)}<option value={k}>{name}</option>{/each}
            </select>
          </label>
          <div class="row">
            <label class="check"><input type="checkbox" checked={board.apwr_h} disabled={busy || (!board.apwr_ok && !board.apwr_h)} onchange={(e) => act(() => bd.set(dev, bd.SET.APWR_H, (e.currentTarget as HTMLInputElement).checked ? 1 : 0))} /> Supply H</label>
            <label class="check"><input type="checkbox" checked={board.apwr_l} disabled={busy || (!board.apwr_ok && !board.apwr_l)} onchange={(e) => act(() => bd.set(dev, bd.SET.APWR_L, (e.currentTarget as HTMLInputElement).checked ? 1 : 0))} /> Supply L</label>
            <label class="check" title="The board's screen"><input type="checkbox" checked={board.lcd_mode !== 0} disabled={busy} onchange={(e) => act(() => bd.set(dev, bd.SET.LCD, (e.currentTarget as HTMLInputElement).checked ? 1 : 0))} /> Screen</label>
          </div>
          <dl>
            <dt>Up</dt><dd>{fmtUptime(board.uptime_ms)}</dd>
            <dt>USB</dt><dd>{board.usb_ss ? 'SuperSpeed' : board.usb_hs ? 'High-Speed' : 'not connected'}</dd>
            <dt>Ethernet</dt><dd>{board.eth_link ? 'link' : 'no cable'}</dd>
            <dt>SD card</dt><dd>{board.sd_card ? (board.sd_blocks ? fmtBytes(board.sd_blocks * 512) : 'in') : 'none'}</dd>
            <dt>FPGA</dt><dd>{board.fpga_done ? `configured${board.fpga_cfg_ms ? ` in ${board.fpga_cfg_ms} ms` : ''}` : 'not configured'} <small class="muted">{hex(board.fpga_id)}</small></dd>
            {#if board.pd_errors || board.pd_hard_resets}<dt>PD errors</dt><dd class="warn">{board.pd_errors} · {board.pd_hard_resets} resets</dd>{/if}
          </dl>
        </div>
      </details>
    {/if}
    {#if error}<p class="error small">{error}</p>{/if}
  </section>
{/if}

{#if showTests && board}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div class="backdrop" role="presentation" onclick={() => !testing && (showTests = false)}>
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div class="dialog" role="dialog" aria-label="Self-test" tabindex="-1" onclick={(e) => e.stopPropagation()}>
      <header>
        <h3>Self-test</h3>
        {#if testing}<span class="muted">running…</span>
        {:else if ran.length}<span class={failed ? 'error' : 'ok'}>{failed ? `${failed} failed` : 'all passed'}</span>{/if}
        <button class="ghost small close" disabled={testing} onclick={() => (showTests = false)}>✕</button>
      </header>
      <table>
        <tbody>
          {#each board.tests as t (t.name)}
            <tr class={testing ? 'none' : t.result}>
              <td class="mark">{testing ? '…' : MARK[t.result]}</td>
              <td><b>{t.name}</b><br /><small class="muted">{t.what}</small></td>
              <td class="det">{testing ? '' : t.result === 'skip' ? 'skipped' : fmtDetail(t)}</td>
            </tr>
          {/each}
        </tbody>
      </table>
      {#if testOk === false && !testing}<p class="small muted">A failure points at that part of the board: see docs/hardware-test.md.</p>{/if}
      <div class="foot">
        <button class="ghost" disabled={testing} onclick={runTests}>Run again</button>
      </div>
    </div>
  </div>
{/if}

<svelte:window onkeydown={(e) => e.key === 'Escape' && showTests && !testing && (showTests = false)} />

<style>
  .fe {
    border: 1px solid var(--border);
    border-left: 3px solid var(--muted);
    border-radius: 6px;
    padding: 6px 8px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .fe.on {
    border-left-color: var(--ok);
  }
  .fe.off {
    border-left-color: var(--warn);
  }
  .fe-head {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .fe-head b {
    white-space: nowrap;
  }
  .fe-head button {
    margin-left: auto;
    white-space: nowrap;
    padding: 2px 10px;
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--muted);
  }
  .fe.on .dot {
    background: var(--ok);
  }
  .fe.off .dot {
    background: var(--warn);
  }
  .fe p {
    margin: 0;
  }
  button.link {
    background: none;
    border: 0;
    padding: 0;
    color: var(--accent);
    text-decoration: underline;
    cursor: pointer;
    font: inherit;
  }
  .more {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-top: 6px;
  }
  .check {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 13px;
  }
  .actions {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-top: 6px;
  }
  .actions .link {
    margin-left: auto;
    color: var(--ok);
  }
  .actions .link.fail {
    color: var(--danger);
  }
  .backdrop {
    position: fixed;
    inset: 0;
    background: #0008;
    display: grid;
    place-items: center;
    z-index: 55;
  }
  .dialog {
    width: min(520px, 92vw);
    max-height: 90vh;
    overflow: auto;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 10px;
    padding: 14px 18px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .dialog header {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .dialog h3 {
    margin: 0;
    font-size: 16px;
  }
  .close {
    margin-left: auto;
  }
  table {
    border-collapse: collapse;
    font-size: 13px;
  }
  td {
    padding: 5px 6px;
    border-top: 1px solid var(--border);
    vertical-align: top;
  }
  td.mark {
    width: 18px;
    font-weight: 700;
  }
  td.det {
    text-align: right;
    white-space: nowrap;
    font-family: ui-monospace, monospace;
    font-size: 12px;
  }
  tr.pass .mark,
  .ok {
    color: var(--ok);
  }
  tr.fail,
  .error {
    color: var(--danger);
  }
  tr.skip .mark,
  tr.none .mark {
    color: var(--muted);
  }
  .foot {
    display: flex;
    justify-content: flex-end;
  }
</style>
