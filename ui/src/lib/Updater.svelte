<script lang="ts">
  // Software update: what runs and what's in the two flash slots; install a
  // firmware image (.nbfw) or a gateware (.bit / .bin); revert the gateware;
  // the ROM bootloader for recovery.
  import { onMount } from 'svelte';
  import type { Device } from './device';
  import { core, hex } from './nbp';
  import * as up from './updater';

  let {
    dev,
    onclose,
    onrestart,
  }: {
    dev: Device;
    onclose: () => void;
    /** the board restarts (a firmware install, recovery): the connection will go */
    onrestart: () => void;
  } = $props();

  let info = $state<up.UpdateInfo | null>(null);
  let unsupported = $state(false);
  let error = $state('');
  let file = $state<{ name: string; kind: 'firmware' | 'gateware'; data: Uint8Array; summary: string } | null>(null);
  let busy = $state(false);
  let stage = $state('');
  let pct = $state(0);
  let result = $state<up.Outcome | null>(null);
  let cancelled = false;

  async function load() {
    error = '';
    try {
      info = await up.info(dev);
      unsupported = !info;
    } catch (e) {
      error = String(e);
    }
  }
  onMount(load);

  async function pick(e: Event) {
    error = '';
    result = null;
    const f = (e.currentTarget as HTMLInputElement).files?.[0];
    (e.currentTarget as HTMLInputElement).value = '';
    if (!f) return;
    const data = new Uint8Array(await f.arrayBuffer());
    if (f.name.toLowerCase().endsWith('.nbfw')) {
      const c = core.firmware_check(data);
      if (!c.ok) {
        error = `${f.name}: ${c.error}`;
        file = null;
        return;
      }
      file = { name: f.name, kind: 'firmware', data, summary: `firmware ${c.version_str}, ${(data.length / 1024).toFixed(0)} KB` };
    } else {
      try {
        const bits = core.bitstream(data);
        file = { name: f.name, kind: 'gateware', data, summary: `gateware, ${(bits.length / 1024).toFixed(0)} KB of bitstream` };
      } catch (err) {
        error = `${f.name}: ${err}`;
        file = null;
      }
    }
  }

  async function installIt() {
    if (!file) return;
    const what = file.kind === 'firmware' ? 'The board restarts to install it.' : 'The board tests it on the FPGA and keeps the old one if it fails.';
    if (!confirm(`Install ${file.name}?\n\n${what}`)) return;
    busy = true;
    cancelled = false;
    error = '';
    result = null;
    try {
      await dev.stop().catch(() => undefined); // board commands answer BUSY while a stream runs
      const progress: up.Progress = (s, d, t) => {
        stage = s;
        pct = t ? Math.round((d / t) * 100) : 0;
      };
      const r = file.kind === 'firmware'
        ? await up.installFirmware(dev, file.data, progress, () => cancelled)
        : await up.installGateware(dev, file.data, progress, () => cancelled);
      result = r;
      if (r.info) info = r.info;
      if (r.ok && file.kind === 'firmware') onrestart();
    } catch (e) {
      error = e instanceof up.Cancelled ? 'Cancelled. The active firmware and gateware are untouched.' : String(e);
    } finally {
      busy = false;
    }
  }

  async function revertIt() {
    if (!confirm('Make the previous gateware (the other slot) active again?')) return;
    busy = true;
    error = '';
    try {
      await dev.stop().catch(() => undefined);
      result = await up.revert(dev);
      if (result.info) info = result.info;
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function recoveryIt() {
    if (!confirm('Restart the board into its ROM USB bootloader (WCH ISP)?\n\nIt leaves this app; flash it with a WCH tool, or power-cycle to come back.')) return;
    await up.recovery(dev);
    onrestart();
  }

  const KIND = ['empty', 'gateware', 'firmware'];
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<div class="backdrop" role="presentation" onclick={() => !busy && onclose()}>
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div class="dialog" role="dialog" aria-label="Software update" tabindex="-1" onclick={(e) => e.stopPropagation()}>
    <header>
      <h2>Software update</h2>
      <button class="ghost small close" onclick={onclose} disabled={busy}>✕</button>
    </header>

    {#if unsupported}
      <p class="warn">This firmware has no software update (no UPDATE command). Update it once with a WCH-Link (see the firmware's README); later updates can come from here.</p>
    {:else if info}
      <dl>
        <dt>Firmware</dt>
        <dd>{info.fw_version_str} <small class="muted">{hex(info.fw_build_id)} · {info.fw_date}</small>{#if info.fw_pending}<small class="warn"> · an install waits for the restart</small>{/if}</dd>
        <dt>Bootloader</dt><dd>{info.boot_version ? `${info.boot_version >> 16}.${(info.boot_version >> 8) & 255}.${info.boot_version & 255}` : 'none'}</dd>
        {#each info.slots as s, k (k)}
          <dt>Slot {k}</dt>
          <dd>
            {KIND[s.kind] ?? '?'}{#if s.kind}: {(s.len / 1024).toFixed(0)} KB, {s.kind === 1 ? `ID ${hex(s.id)}` : `v${s.id >> 16}.${(s.id >> 8) & 255}.${s.id & 255}`}{/if}
            {#if k === info.active}<b class="tag">active gateware</b>{/if}
            {#if k === info.update_slot}<span class="tag muted">update slot</span>{/if}
          </dd>
        {/each}
        {#if info.installs}<dt>Installs</dt><dd>{info.installs}</dd>{/if}
        {#if info.msg}<dt>Last</dt><dd>{info.msg}</dd>{/if}
      </dl>

      <div class="pick">
        <label class="file">
          <input type="file" accept=".nbfw,.bit,.bin" onchange={pick} disabled={busy} />
          <span>Choose a firmware (.nbfw) or gateware (.bit / .bin)…</span>
        </label>
        {#if file}
          <div class="chosen">
            <b>{file.name}</b> <span class="muted">{file.summary}</span>
            <button class="primary" onclick={installIt} disabled={busy}>Install</button>
          </div>
        {/if}
      </div>

      {#if busy}
        <div class="prog">
          <div class="bar"><div style="width: {pct}%"></div></div>
          <span>{stage} {pct}%</span>
          <button class="ghost small" onclick={() => (cancelled = true)}>Cancel</button>
        </div>
      {/if}

      {#if result}
        <p class={result.ok ? 'ok' : 'error'}>
          {result.ok ? '✓' : '✕'} {result.msg || core.status_name(result.status)}
          {#if result.ok && file?.kind === 'firmware'}<br /><small>The board is restarting to install it: connect again in a few seconds.</small>{/if}
        </p>
      {/if}

      <div class="more">
        <button class="ghost small" onclick={revertIt} disabled={busy || !info.slots.every((s) => s.kind === 1)} title="The gateware in the other slot active again">Previous gateware</button>
        <button class="ghost small" onclick={recoveryIt} disabled={busy} title="Restart into the H417's ROM USB bootloader">Recovery bootloader…</button>
      </div>
    {:else if !error}
      <p class="muted">Asking the board…</p>
    {/if}

    {#if error}<p class="error">{error}</p>{/if}
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: #0008;
    display: grid;
    place-items: center;
    z-index: 50;
  }
  .dialog {
    width: min(620px, 92vw);
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 10px;
    padding: 14px 18px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  header {
    display: flex;
    align-items: center;
  }
  h2 {
    margin: 0;
    font-size: 16px;
  }
  .close {
    margin-left: auto;
  }
  dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 4px 14px;
    margin: 0;
    font-size: 13px;
  }
  dt {
    color: var(--muted);
  }
  dd {
    margin: 0;
  }
  .tag {
    font-size: 11px;
    margin-left: 6px;
    padding: 0 5px;
    border: 1px solid var(--border);
    border-radius: 3px;
  }
  b.tag {
    border-color: var(--ok);
    color: var(--ok);
  }
  .pick {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .file input {
    display: none;
  }
  .file span {
    display: inline-block;
    padding: 8px 12px;
    border: 1px dashed var(--border);
    border-radius: 6px;
    cursor: pointer;
  }
  .chosen {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .chosen button {
    margin-left: auto;
  }
  .prog {
    display: flex;
    align-items: center;
    gap: 10px;
    font-size: 13px;
  }
  .bar {
    flex: 1;
    height: 8px;
    border-radius: 4px;
    background: var(--border);
    overflow: hidden;
  }
  .bar div {
    height: 100%;
    background: var(--accent);
    transition: width 0.2s;
  }
  .more {
    display: flex;
    gap: 8px;
    justify-content: flex-end;
  }
  .ok {
    color: var(--ok);
  }
  .error {
    color: var(--danger);
  }
  .warn {
    color: var(--warn);
  }
  .muted {
    color: var(--muted);
  }
  p {
    margin: 0;
  }
  button.small {
    padding: 2px 8px;
    font-size: 12px;
  }
</style>
