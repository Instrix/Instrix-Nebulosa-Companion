<script lang="ts">
  // The Debug tab (builds with NEBULOSA_DEBUG=1): what the Nebulosa Debugger
  // (Instrix-Nebulosa-Debugger) does, inside the Companion. The device's and
  // the host's stream counters, the range switch (outputs, comparators,
  // timing, forced comparators), the RS1473 ADC link, the FPGA registers,
  // the board's low-level controls, the SPI flash, round trips and raw NBP
  // commands, and a log of what was sent.
  import { onDestroy, onMount } from 'svelte';
  import { ask } from './ask';
  import * as bd from './board';
  import * as dbg from './debug';
  import { isNative, type Device } from './device';
  import * as native from './native';
  import { core, hex, type DataStats, type Info } from './nbp';
  import { DemoLink } from './demo';
  import { bringup } from './bringup';
  import BringupView from './bringup/BringupView.svelte';

  let {
    dev,
    info,
    streaming,
    stats,
    throughput,
    board,
    onstart,
    onstartadc,
    onstop,
    onnote,
  }: {
    dev: Device | null;
    info: Info | null;
    streaming: boolean;
    /** the host's counters (Meta.stats) */
    stats: DataStats | null;
    /** bytes/s */
    throughput: number;
    board: bd.BoardStatus | null;
    onstart: () => Promise<void>;
    /** starts with the ADC as the source (the bring-up) */
    onstartadc: () => Promise<void>;
    onstop: () => Promise<void>;
    onnote: (m: string) => void;
  } = $props();

  /** Tools (the debugger's panels) or the AFE bring-up */
  let sub = $state<'tools' | 'bringup'>(loadSub());
  function loadSub(): 'tools' | 'bringup' {
    try {
      return localStorage.getItem('nebulosa.debugSub') === 'bringup' ? 'bringup' : 'tools';
    } catch {
      return 'tools';
    }
  }
  function setSub(s: 'tools' | 'bringup') {
    sub = s;
    try {
      localStorage.setItem('nebulosa.debugSub', s);
    } catch {
      /* storage unavailable */
    }
  }

  // the bring-up reads the current props through this
  const bu = bringup({
    dev: () => dev,
    streaming: () => streaming,
    startAdc: () => onstartadc(),
    log: (t, bad) => logf(t, bad),
    demoBench: (x) => {
      if (dev?.link instanceof DemoLink) dev.link.setBench(x);
    },
  });
  // the Nebulosa gone: the bring-up switches its current off
  $effect(() => {
    if (!dev) bu.linkLost();
  });

  // ---- log ------------------------------------------------------------------------
  let log = $state<{ t: number; text: string; bad: boolean }[]>([]);
  function logf(text: string, bad = false) {
    log = [...log.slice(-299), { t: Date.now(), text, bad }];
  }
  const logLine = (l: { t: number; text: string; bad: boolean }) => `${new Date(l.t).toLocaleTimeString()}${l.bad ? ' !' : ''} ${l.text}`;
  /** one line, or the whole log (oldest first), to the clipboard */
  async function copyLog(l?: { t: number; text: string; bad: boolean }) {
    const text = l ? logLine(l) : log.map(logLine).join('\n');
    try {
      await navigator.clipboard.writeText(text);
      onnote(l ? 'Log line copied' : `Log copied (${log.length} lines)`);
    } catch {
      onnote('Could not copy');
    }
  }
  /** runs a command, logs what happened; null when it failed */
  async function act<T>(what: string, f: () => Promise<T>, quiet = false): Promise<T | null> {
    if (!dev) return null;
    try {
      const r = await f();
      if (!quiet) logf(`${what}: OK`);
      return r;
    } catch (e) {
      logf(`${what}: ${dbg.statusText(e)}`, true);
      return null;
    }
  }

  // ---- polled: the device's counters and the range ------------------------------------
  let devStats = $state<ReturnType<typeof core.parse_stats> | null>(null);
  let rng = $state<Awaited<ReturnType<typeof dbg.range>>>(null);
  let clk = $state(dbg.CLK_HZ_OLD);
  let adc = $state<dbg.AdcLink | null>(null);
  let adcLive = $state(true);
  let polling = false;
  /** a long operation (sweep, flash) owns the device: no polling meanwhile */
  let busy = $state('');
  let progress = $state(0);

  const hasAdc = $derived(!!info && info.fpga_id >= dbg.FPGA_ID_RS1473);

  async function poll() {
    if (!dev || polling || busy || sub !== 'tools') return;
    polling = true;
    try {
      devStats = core.parse_stats(await dev.cmd(dbg.CMD.GET_STATS)) ?? null;
      rng = await dbg.range(dev).catch(() => null);
      if (hasAdc && adcLive) adc = await dbg.adcLink(dev).catch(() => null);
    } catch {
      /* the next one */
    } finally {
      polling = false;
    }
    pushHistory();
  }

  let timer: ReturnType<typeof setInterval> | undefined;
  let lastDev: Device | null = null;
  $effect(() => {
    // a new device: read what it runs now
    if (dev && dev !== lastDev) {
      lastDev = dev;
      regs = null;
      void (async () => {
        clk = await dbg.clkHz(dev!, info?.fpga_id ?? 0).catch(() => dbg.CLK_HZ_OLD);
        await poll();
        loadTiming();
      })();
    }
    if (!dev) lastDev = null;
  });
  onMount(() => {
    timer = setInterval(poll, 500);
    window.addEventListener('keydown', onKey);
  });
  onDestroy(() => {
    clearInterval(timer);
    window.removeEventListener('keydown', onKey);
  });

  // ---- throughput history ----------------------------------------------------------
  let histMB: number[] = [];
  let histConv: number[] = [];
  let lastConv = { t: 0, n: 0 };
  let canvas = $state<HTMLCanvasElement>();
  function pushHistory() {
    const now = performance.now();
    const n = stats?.stream.samples ?? 0;
    const cps = lastConv.t && n >= lastConv.n ? ((n - lastConv.n) * 1000) / (now - lastConv.t) : 0;
    lastConv = { t: now, n };
    histMB = [...histMB, throughput / 1e6].slice(-240);
    histConv = [...histConv, cps / 1e6].slice(-240);
    convRate = cps;
    draw();
  }
  let convRate = $state(0);
  function draw() {
    const c = canvas;
    if (!c) return;
    const w = c.clientWidth,
      h = c.clientHeight;
    const dpr = window.devicePixelRatio || 1;
    if (c.width !== Math.round(w * dpr)) c.width = Math.round(w * dpr);
    if (c.height !== Math.round(h * dpr)) c.height = Math.round(h * dpr);
    const g = c.getContext('2d')!;
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    g.clearRect(0, 0, w, h);
    const css = getComputedStyle(c);
    const line = (ys: number[], color: string) => {
      const max = Math.max(1e-9, ...histMB, ...histConv) * 1.15;
      g.strokeStyle = color;
      g.lineWidth = 1.5;
      g.beginPath();
      ys.forEach((y, i) => {
        const x = w - (ys.length - 1 - i) * (w / 239);
        const yy = h - (y / max) * (h - 4) - 2;
        i ? g.lineTo(x, yy) : g.moveTo(x, yy);
      });
      g.stroke();
    };
    line(histMB, css.getPropertyValue('--accent') || '#2f80c0');
    line(histConv, css.getPropertyValue('--warn') || '#e0a84a');
  }

  // ---- range ---------------------------------------------------------------------------
  let tim = $state({ up_filt: 2, down_filt: 16, overlap: 26, hold: 27, settle: 40, align: true, dirty_up: true, dirty_down: true });
  let force = $state(false);
  let forceBits = $state([false, false, false, false, false, false]);
  let autoMin = $state(0);
  let autoMax = $state(5);
  function loadTiming() {
    const d = rng?.dec;
    if (!d) return;
    tim = { up_filt: d.up_filt, down_filt: d.down_filt, overlap: d.overlap, hold: d.hold, settle: d.settle, align: d.align, dirty_up: d.dirty_up, dirty_down: d.dirty_down };
    autoMin = d.min;
    autoMax = d.max;
    force = d.forced;
  }
  const cycUs = $derived(1e6 / clk);
  const forceMask = () => forceBits.reduce((m, b, i) => m | (b ? 1 << i : 0), 0);

  async function setRange(automatic: boolean, manual: number) {
    await act(automatic ? `range automatic ${autoMin}..${autoMax}` : `range manual ${manual}`, async () => {
      await dev!.setRange(automatic, manual, autoMin, Math.max(autoMin, autoMax));
      // SET_RANGE rewrites RANGE_CTRL and clears the force bits: put them back
      if (force) await dbg.setRangeForce(dev!, true, forceMask());
    });
    void poll();
  }
  async function applyTiming() {
    await act(`range timing (UP ${tim.up_filt}, DOWN ${tim.down_filt}, overlap ${tim.overlap}, hold ${tim.hold}, settle ${tim.settle})`, () => dbg.setRangeTiming(dev!, tim));
    void poll();
  }
  async function applyForce() {
    await act(`force comparators ${force ? 'on 0x' + forceMask().toString(16) : 'off'}`, () => dbg.setRangeForce(dev!, force, forceMask()));
    void poll();
  }

  // ---- ADC link --------------------------------------------------------------------------
  let phA = $state(0);
  let phB = $state(0);
  let sweepDwell = $state(300);
  let sweep = $state<{ steps: dbg.SweepStep[]; good: [number, number] | null } | null>(null);

  async function recal() {
    await act(
      'ADC RECAL',
      async () => {
        await dbg.adcRecal(dev!);
        const ms = await dbg.adcWaitLock(dev!, 500);
        logf(ms === null ? 'ADC RECAL: no lock within 500 ms' : `ADC RECAL: locked after ${ms.toFixed(1)} ms`, ms === null);
      },
      true,
    );
    void poll();
  }
  async function phases(manual: boolean) {
    await act(manual ? `ADC phases A ${phA}, B ${phB}` : 'ADC phases: measured', () => dbg.adcPhases(dev!, manual ? [phA, phB] : null));
    void poll();
  }
  async function engine(echo: boolean) {
    await act(`ADC engine ${echo ? 'echo-clocked (DCM)' : 'clk_fast'}`, async () => {
      await dbg.adcEngine(dev!, echo);
      const ms = await dbg.adcWaitLock(dev!, 100);
      logf(ms === null ? '  no lock within 100 ms' : `  locked after ${ms.toFixed(1)} ms`, ms === null);
    });
    void poll();
  }
  async function runSweep() {
    if (!dev || busy) return;
    busy = 'phase sweep';
    sweep = { steps: [], good: null };
    try {
      const r = await dbg.adcSweep(dev, sweepDwell, (s) => {
        sweep = { steps: [...sweep!.steps, s], good: null };
        progress = (s.phase + 1) / dbg.ADC_PHASES;
        logf(`  phase ${s.phase} (${s.phase * 5} ns): A ${s.held[0] ? 'locked' : '-'}, B ${s.held[1] ? 'locked' : '-'}, ADC_ERRORS +${s.errors}`);
      });
      sweep = { steps: sweep.steps, good: r.good };
      logf(`phase sweep: good A ${maskList(r.good[0])}, B ${maskList(r.good[1])}`);
    } catch (e) {
      logf(`phase sweep: ${dbg.statusText(e)}`, true);
    } finally {
      busy = '';
    }
    void poll();
  }
  const maskList = (m: number) => Array.from({ length: dbg.ADC_PHASES }, (_, k) => k).filter((k) => (m >> k) & 1).join(' ') || '—';

  // ---- registers -----------------------------------------------------------------------
  let regs = $state<(number | null)[] | null>(null);
  let regIdx = $state(31);
  let regVal = $state('0x00000000');
  async function readRegs() {
    if (!dev) return;
    regs = await dbg.readAll(dev);
    logf(`registers: read ${regs.filter((r) => r !== null).length} of ${dbg.FPGA_REGS}`);
  }
  async function writeReg() {
    const v = Number(regVal.trim());
    if (!Number.isFinite(v) || v < 0 || v > 0xffffffff) return logf(`register write: "${regVal}" is not a 32-bit value`, true);
    const i = Math.max(0, Math.min(dbg.FPGA_REGS - 1, Math.round(regIdx)));
    if (await act(`register ${i} (${core.reg_name(i) || '—'}) ← ${hex(v)}`, () => dbg.regWrite(dev!, i, v)) !== null) {
      const back = await dbg.regRead(dev!, i).catch(() => null);
      if (regs) regs[i] = back;
      if (back !== null) logf(`  reads back ${hex(back)}`);
    }
  }
  function pickReg(i: number) {
    regIdx = i;
    if (regs?.[i] != null) regVal = hex(regs[i]!);
  }

  // ---- board -----------------------------------------------------------------------------
  const bf = (f: number) => !!board && (board.flags & f) !== 0;
  async function bset(what: string, item: number, value: number) {
    await act(what, () => bd.set(dev!, item, value));
  }
  function setLed(i: number, on: boolean) {
    if (!board) return;
    const leds = on ? board.leds | (1 << i) : board.leds & ~(1 << i);
    void bset(`LED${i} ${on ? 'on' : 'off'}`, bd.SET.LEDS, 0x100 | leds);
  }
  let keys = $state(false);
  const KEYMAP: Record<string, number> = { ArrowUp: dbg.KEY.UP, ArrowDown: dbg.KEY.DOWN, ArrowLeft: dbg.KEY.LEFT, ArrowRight: dbg.KEY.RIGHT, Enter: dbg.KEY.ENTER, Backspace: dbg.KEY.BACK, Escape: dbg.KEY.BACK };
  function uiKey(k: number) {
    if (!dev) return;
    bd.set(dev, bd.SET.UI_KEY, k).catch((e) => logf(`UI key: ${bd.reason(e)}`, true));
  }
  function onKey(e: KeyboardEvent) {
    if (!keys || !dev) return;
    const t = e.target as HTMLElement | null;
    if (t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.tagName === 'SELECT' || t.isContentEditable)) return;
    const k = KEYMAP[e.key];
    if (!k) return;
    e.preventDefault();
    e.stopPropagation();
    uiKey(k);
  }

  // ---- flash -----------------------------------------------------------------------------
  let flAddr = $state('0x0F0000');
  let flLen = $state('4096');
  let flData = $state<{ addr: number; bytes: Uint8Array } | null>(null);
  let flFile = $state<{ name: string; bytes: Uint8Array } | null>(null);
  const num = (s: string) => Number(s.trim());
  function flashArgs(): { addr: number; len: number } | null {
    const addr = num(flAddr),
      len = num(flLen);
    const size = board?.flash_bytes || 1 << 20;
    if (!Number.isInteger(addr) || addr < 0 || addr >= size) return logf(`flash: address ${flAddr} outside the ${size >> 10} KB flash`, true), null;
    if (!Number.isInteger(len) || len <= 0 || addr + len > size) return logf(`flash: length ${flLen} runs past the end`, true), null;
    return { addr, len };
  }
  async function long<T>(what: string, f: () => Promise<T>): Promise<T | null> {
    if (!dev || busy) return null;
    busy = what;
    progress = 0;
    try {
      return await act(what, f);
    } finally {
      busy = '';
    }
  }
  async function flashRead() {
    const a = flashArgs();
    if (!a) return;
    const b = await long(`flash read 0x${a.addr.toString(16)} +${a.len}`, () => dbg.flashRead(dev!, a.addr, a.len, (d) => (progress = d / a.len)));
    if (b) flData = { addr: a.addr, bytes: b };
  }
  async function flashSave() {
    if (!flData) return;
    const name = `flash-${flData.addr.toString(16).padStart(6, '0')}.bin`;
    if (isNative) {
      const path = await native.pickSave(name, 'Binary', 'bin');
      if (!path) return;
      await native.saveBytes(path, flData.bytes);
      onnote(`Saved ${flData.bytes.length} bytes to ${path}`);
    } else {
      const a = document.createElement('a');
      a.href = URL.createObjectURL(new Blob([flData.bytes as BlobPart]));
      a.download = name;
      a.click();
      setTimeout(() => URL.revokeObjectURL(a.href), 2000);
    }
  }
  async function pickFlashFile(e: Event) {
    const el = e.currentTarget as HTMLInputElement;
    const f = el.files?.[0];
    el.value = '';
    if (f) flFile = { name: f.name, bytes: new Uint8Array(await f.arrayBuffer()) };
  }
  async function flashErase() {
    const a = flashArgs();
    if (!a) return;
    if (a.addr % dbg.FLASH_SECTOR) return logf('flash erase: the address must be 4 KB aligned', true);
    const n = Math.ceil(a.len / dbg.FLASH_SECTOR) * dbg.FLASH_SECTOR;
    if (!(await ask(`Erase ${n} bytes of the board's SPI flash from 0x${a.addr.toString(16)}?\n\nThe board refuses its running gateware and boot record (PROTECTED).`))) return;
    await long(`flash erase 0x${a.addr.toString(16)} +${n}`, () => dbg.flashErase(dev!, a.addr, n));
  }
  async function flashWrite() {
    const f = flFile;
    const addr = num(flAddr);
    if (!f) return;
    if (!Number.isInteger(addr) || addr % dbg.FLASH_SECTOR) return logf('flash write: the address must be 4 KB aligned (it erases first)', true);
    const size = board?.flash_bytes || 1 << 20;
    if (addr + f.bytes.length > size) return logf(`flash write: ${f.name} (${f.bytes.length} bytes) runs past the end`, true);
    if (!(await ask(`Erase and write ${f.name} (${f.bytes.length} bytes) to the board's SPI flash at 0x${addr.toString(16)}, then read it back?`))) return;
    await long(`flash write ${f.name} at 0x${addr.toString(16)}`, async () => {
      await dbg.flashErase(dev!, addr, f.bytes.length);
      await dbg.flashWrite(dev!, addr, f.bytes, (d) => (progress = (0.5 * d) / f.bytes.length));
      const back = await dbg.flashRead(dev!, addr, f.bytes.length, (d) => (progress = 0.5 + (0.5 * d) / f.bytes.length));
      const bad = back.findIndex((b, i) => b !== f.bytes[i]);
      if (bad >= 0) throw new Error(`verify: differs at 0x${(addr + bad).toString(16)}`);
      logf(`  verified ${f.bytes.length} bytes`);
    });
  }

  // ---- link ------------------------------------------------------------------------------
  let pings = $state<{ size: number; r: { min: number; mean: number; max: number; bad: number } }[]>([]);
  async function runPing() {
    const out: typeof pings = [];
    for (const size of [16, 504]) {
      const r = await act(`PING ${size} B ×50`, () => dbg.ping(dev!, size, 50), true);
      if (r) {
        out.push({ size, r });
        logf(`PING ${size} B: ${r.min.toFixed(2)} / ${r.mean.toFixed(2)} / ${r.max.toFixed(2)} ms (min / mean / max)${r.bad ? `, ${r.bad} bad echoes` : ''}`, r.bad > 0);
      }
    }
    pings = out;
  }

  // ---- raw command -----------------------------------------------------------------------
  let rawCmd = $state('0x22');
  let rawPayload = $state('');
  let rawOut = $state<{ status: number; payload: Uint8Array; ms: number } | null>(null);
  async function sendRaw() {
    if (!dev) return;
    const c = Number(rawCmd.trim());
    if (!Number.isInteger(c) || c < 0 || c > 0x7f) return logf('raw: the command is a byte 0x00..0x7F', true);
    let p: Uint8Array;
    try {
      p = dbg.parseHex(rawPayload);
    } catch (e) {
      return logf(`raw: ${dbg.statusText(e)}`, true);
    }
    if ((c === dbg.CMD.FLASH_ERASE || c === dbg.CMD.FLASH_WRITE || c === dbg.CMD.RESET) && !(await ask(`Send command ${hex(c, 2)}? It changes the board (flash / reset).`))) return;
    try {
      rawOut = await dbg.raw(dev, c, p);
      logf(`raw ${hex(c, 2)} [${p.length} B] → ${core.status_name(rawOut.status)}, ${rawOut.payload.length} B, ${rawOut.ms.toFixed(2)} ms`, rawOut.status !== 0);
    } catch (e) {
      logf(`raw ${hex(c, 2)}: ${dbg.statusText(e)}`, true);
    }
  }

  async function resetDevice() {
    if (!(await ask('Reset the device? It restarts (~50 ms later) and the app reconnects.'))) return;
    await act('RESET', () => dev!.cmd(dbg.CMD.RESET));
  }

  const LINK = ['none', 'USB FS', 'USB HS', 'TCP', 'USB SS'];
  const SRC = ['stub random', 'stub pattern', 'ADC'];
  const fmtN = (n: number | undefined) => (n ?? 0).toLocaleString();
  const ip = (v: number) => (v ? [v & 255, (v >> 8) & 255, (v >> 16) & 255, (v >>> 24) & 255].join('.') : '—');
</script>

{#snippet logPane(cls: string, empty: string)}
  <section class="logsec {cls}">
    <h3>
      Log
      <button class="ghost small" onclick={() => copyLog()} disabled={!log.length} title="Copy the whole log, oldest first">Copy all</button>
      <button class="ghost small" onclick={() => (log = [])}>Clear</button>
    </h3>
    <div class="log">
      {#each log.slice().reverse() as l, i (log.length - i)}
        <div class="line" class:bad={l.bad}>
          <span class="muted">{new Date(l.t).toLocaleTimeString()}</span> {l.text}
          <button class="ghost small copy" onclick={() => copyLog(l)} title="Copy this line" aria-label="Copy this line">⧉</button>
        </div>
      {/each}
      {#if !log.length}<div class="muted">{empty}</div>{/if}
    </div>
  </section>
{/snippet}


<div class="debug">
  {#if !dev}
    <p class="empty">Connect a device (or open the demo) to debug it.</p>
  {:else}
    <div class="bar">
      <strong>Debug</strong>
      <span class="muted">{dev.link.description}{info ? ` · firmware ${info.fw_version_str} · FPGA ${hex(info.fpga_id)}` : ''}</span>
      <span class="sp"></span>
      {#if busy}<span class="busy">{busy} <progress value={progress}></progress></span>{/if}
      {#if streaming}
        <button onclick={onstop}>Stop</button>
      {:else}
        <button class="primary" onclick={onstart}>Start</button>
      {/if}
      <button class="ghost danger-text" onclick={resetDevice} disabled={!!busy}>Reset device</button>
    </div>

    <div class="subtabs">
      <button class:on={sub === 'tools'} onclick={() => setSub('tools')}>Tools</button>
      <button class:on={sub === 'bringup'} onclick={() => setSub('bringup')}>AFE bring-up{bu.busy ? ' …' : ''}{bu.loadIn || bu.psuOut[0] ? ' ⚡' : ''}</button>
    </div>

    {#if sub === 'bringup'}
      <BringupView b={bu} connected={!!dev} {streaming} {onnote} />
      {@render logPane('bulog', 'Nothing yet.')}
    {:else}
    <div class="grid">
      <!-- stream -->
      <section>
        <h3>Stream</h3>
        <table class="kv">
          <tbody>
            <tr><th>throughput</th><td>{(throughput / 1e6).toFixed(2)} MB/s</td><th>conversions</th><td>{(convRate / 1e6).toFixed(3)} M/s</td></tr>
            <tr><th>NBP blocks</th><td>{fmtN(stats?.blocks)}</td><th>lost blocks</th><td class:bad={!!stats?.lost_blocks}>{fmtN(stats?.lost_blocks)}</td></tr>
            <tr><th>H417 stalls</th><td class:bad={!!stats?.stall_flags}>{fmtN(stats?.stall_flags)}</td><th>gap flags</th><td class:bad={!!stats?.gap_flags}>{fmtN(stats?.gap_flags)}</td></tr>
            <tr><th>samples</th><td>{fmtN(stats?.stream.samples)}</td><th>DIRTY</th><td>{fmtN(stats?.stream.dirty)} ({stats?.stream.samples ? ((100 * stats.stream.dirty) / stats.stream.samples).toFixed(3) : '0'} %)</td></tr>
            <tr><th>FPGA blocks</th><td>{fmtN(stats?.stream.blocks)}</td><th>FPGA seq errors</th><td class:bad={!!stats?.stream.seq_errors}>{fmtN(stats?.stream.seq_errors)}</td></tr>
            <tr><th>OVF blocks</th><td class:bad={!!stats?.stream.ovf_blocks}>{fmtN(stats?.stream.ovf_blocks)}</td><th>LATE</th><td>{fmtN(stats?.stream.late)}</td></tr>
            <tr><th>gaps (time)</th><td class:bad={!!stats?.stream.gap_periods}>{((stats?.stream.gap_periods ?? 0) / 2e6).toFixed(6)} s</td><th>malformed</th><td class:bad={!!stats?.stream.malformed}>{fmtN(stats?.stream.malformed)}</td></tr>
            <tr><th>range changes</th><td>{fmtN(stats?.stream.range_changes)}</td><th>resyncs</th><td>{fmtN(stats?.resyncs)}</td></tr>
            <tr><th>link-test blocks</th><td>{fmtN(stats?.test_blocks)}</td><th>bad test blocks</th><td class:bad={!!stats?.bad_blocks}>{fmtN(stats?.bad_blocks)}</td></tr>
          </tbody>
        </table>
        <canvas bind:this={canvas} class="spark"></canvas>
        <div class="legend"><span class="l1">MB/s</span> <span class="l2">M conv/s</span> <span class="muted">last 2 min</span></div>
      </section>

      <!-- device -->
      <section>
        <h3>Device</h3>
        {#if devStats}
          <table class="kv">
            <tbody>
              <tr><th>uptime</th><td>{(devStats.uptime_ms / 1e3).toFixed(1)} s</td><th>stream link</th><td>{LINK[devStats.stream_link] ?? devStats.stream_link}</td></tr>
              <tr><th>blocks sent</th><td>{fmtN(devStats.blocks)}</td><th>words sent</th><td>{fmtN(devStats.words)}</td></tr>
              <tr><th>H417 stalls</th><td class:bad={!!devStats.stalls}>{fmtN(devStats.stalls)}</td><th>FPGA drops</th><td class:bad={!!devStats.fpga_drops}>{fmtN(devStats.fpga_drops)}</td></tr>
              <tr><th>FIFO level</th><td>{fmtN(devStats.fifo_level)}</td><th>DECIM</th><td class="mono">{hex(devStats.decim)}</td></tr>
              <tr><th>ADC_CTRL</th><td class="mono">{hex(devStats.adc_ctrl)}</td><th>source</th><td>{SRC[devStats.adc_ctrl & 3] ?? '?'}</td></tr>
              <tr><th>Ethernet</th><td>{devStats.eth & 1 ? 'link' : 'no link'}{devStats.eth & 4 ? ', client' : ''}</td><th>IP</th><td class="mono">{ip(devStats.eth_ip)}</td></tr>
            </tbody>
          </table>
        {:else}
          <p class="muted">No GET_STATS answer.</p>
        {/if}
        {#if info}
          <p class="muted small">Protocol {info.proto_version}, base rate {(info.base_rate_hz / 1e6).toFixed(2)} MS/s, {info.block_words} words a block, caps {hex(info.caps, 2)}, FPGA clock {(clk / 1e6).toFixed(0)} MHz{info.build ? `, built ${info.build}` : ''}</p>
        {/if}
        <h4>Round trips</h4>
        <div class="row">
          <button onclick={runPing} disabled={!!busy}>PING 16 B / 504 B ×50</button>
        </div>
        {#each pings as p (p.size)}
          <div class="mono small">{String(p.size).padStart(3)} B  {p.r.min.toFixed(2)} / {p.r.mean.toFixed(2)} / {p.r.max.toFixed(2)} ms{p.r.bad ? `  ${p.r.bad} bad` : ''}</div>
        {/each}
        <p class="muted small">min / mean / max, from the app (its own IPC included)</p>
      </section>

      <!-- range -->
      <section>
        <h3>Current range</h3>
        {#if rng}
          {@const d = rng.dec}
          <p>Range <strong>{d.range}</strong> ({dbg.RANGE_FS[d.range] ?? '?'} full scale){d.switching ? ' · switching' : ''} · {fmtN(rng.raw.count)} switches</p>
          <div class="row">
            <span class="lbl">EN_TAP</span>
            {#each dbg.RANGE_FS as fs, i (i)}
              <button class="tap" class:on={(d.taps >> i) & 1} title={`range ${i}, ${fs} full scale: click to select it (manual)`} onclick={() => setRange(false, i)}>{i}</button>
            {/each}
          </div>
          <div class="row">
            <span class="lbl">comp</span>
            {#each dbg.COMP_NAMES as c, i (i)}
              <span class="comp" class:on={(d.comps >> i) & 1} title={`${c}: ${(d.comps >> i) & 1 ? 'asserted' : 'not asserted'}`}>{c}</span>
            {/each}
          </div>
          <p class="muted small">Device: {d.automatic ? `automatic, ranges ${d.min}..${d.max}` : `manual, range ${d.manual}`}{d.forced ? ', comparators forced' : ''}. The Acquisition settings apply again at the next Start.</p>
          <div class="row">
            <button onclick={() => setRange(true, d.manual)}>Automatic</button>
            <label>min <input type="number" min="0" max="5" bind:value={autoMin} /></label>
            <label>max <input type="number" min="0" max="5" bind:value={autoMax} /></label>
          </div>
          {#if d.automatic && !d.comps && !d.forced}
            <p class="muted small">No comparator asserted (no AFE?): automatic ranging stays put. Force them below to test it.</p>
          {/if}

          <details>
            <summary>Timing (clk cycles, {(cycUs * 1e3).toFixed(2)} ns = {(clk / 1e6).toFixed(0)} MHz)</summary>
            <div class="form">
              <label>UP filter <input type="number" min="0" max="63" bind:value={tim.up_filt} /></label>
              <label>DOWN filter <input type="number" min="0" max="63" bind:value={tim.down_filt} /></label>
              <label>overlap <input type="number" min="0" max="255" bind:value={tim.overlap} /> <span class="muted">{((tim.overlap + 1) * cycUs).toFixed(2)} µs</span></label>
              <label>hold <input type="number" min="0" max="4095" bind:value={tim.hold} /> <span class="muted">{(tim.hold * cycUs).toFixed(2)} µs</span></label>
              <label>settle <input type="number" min="0" max="4095" bind:value={tim.settle} /> <span class="muted">{(tim.settle * cycUs).toFixed(2)} µs dirty after MAKE</span></label>
              <label class="check" title="Switch right after an ADC sampling instant: the switch has a whole sample period to settle"><input type="checkbox" bind:checked={tim.align} /> align to samples</label>
              <label class="check"><input type="checkbox" bind:checked={tim.dirty_up} /> dirty while UP</label>
              <label class="check"><input type="checkbox" bind:checked={tim.dirty_down} /> dirty while DOWN</label>
            </div>
            <div class="row"><button onclick={applyTiming}>Apply timing</button><button class="ghost" onclick={loadTiming}>Read back</button></div>
          </details>
          <details>
            <summary>Force comparators (test without the AFE)</summary>
            <p class="muted small">Replaces the comparator inputs; only automatic mode acts on them. UP moves to a less sensitive range, DOWN to a more sensitive one, one step per hold time while asserted.</p>
            <div class="row wrap">
              <label class="check"><input type="checkbox" bind:checked={force} onchange={applyForce} /> force</label>
              {#each dbg.COMP_NAMES as c, i (i)}
                <label class="check"><input type="checkbox" bind:checked={forceBits[i]} onchange={applyForce} /> {c}</label>
              {/each}
            </div>
          </details>
        {:else}
          <p class="muted">No GET_RANGE answer (firmware without range control?).</p>
        {/if}
      </section>

      <!-- ADC link -->
      {#if hasAdc}
        <section>
          <h3>RS1473 ADC link</h3>
          <div class="row">
            <label class="check"><input type="checkbox" bind:checked={adcLive} /> live</label>
            <span class="sp"></span>
            <button onclick={recal} disabled={!!busy}>RECAL</button>
          </div>
          {#if adc}
            <p>
              <span class:ok={adc.locked} class:bad={!adc.locked}>{adc.locked ? 'locked' : 'not locked'}</span> ·
              engine {adc.echo ? 'echo-clocked (DCM)' : 'clk_fast'} · phases {adc.manual ? 'manual' : 'measured'}{adc.hold ? ' · HOLD' : ''} · source {SRC[adc.source] ?? '?'} ·
              ADC_ERRORS <span class:bad={!!adc.errors}>{fmtN(adc.errors)}</span>
            </p>
            <table class="grp">
              <thead><tr><th>group</th><th>lock</th><th>phase</th><th>change map</th><th>eye</th><th>{adc.echo ? 'DCM' : 'round trip'}</th></tr></thead>
              <tbody>
                {#each adc.groups as g, k (k)}
                  <tr>
                    <td>{k ? 'B (U8: chain C, voltage)' : 'A (U7: chains A, B)'}</td>
                    <td class:ok={g.locked} class:bad={!g.locked}>{g.locked ? 'yes' : 'no'}</td>
                    <td>{g.phase} ({g.phase * 5} ns)</td>
                    <td class="mono" title="x: a line changes at that phase; [ ]: the phase in use">{dbg.mapStr(g.tmap, g.phase)}</td>
                    <td>{(g.eye + 1) * 5} ns</td>
                    <td>{adc.echo ? (g.dcm_locked ? 'locked' : 'not locked') : `${g.delay} cyc (${(g.delay * 5).toFixed(0)} ns)`}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
            <p class="muted small mono">ADC_CTRL {hex(adc.ctrl)} · TIMING {hex(adc.timing)} · DELAY {hex(adc.delay)}</p>
          {:else}
            <p class="muted">No answer from registers 31, 42..44.</p>
          {/if}
          <div class="row wrap">
            <span class="lbl">phases</span>
            <label>A <select bind:value={phA}>{#each [0, 1, 2, 3, 4] as p (p)}<option value={p}>{p} ({p * 5} ns)</option>{/each}</select></label>
            <label>B <select bind:value={phB}>{#each [0, 1, 2, 3, 4] as p (p)}<option value={p}>{p} ({p * 5} ns)</option>{/each}</select></label>
            <button onclick={() => phases(true)} disabled={!!busy}>Fixed</button>
            <button onclick={() => phases(false)} disabled={!!busy}>Measured</button>
          </div>
          <div class="row">
            <span class="lbl">engine</span>
            <button class:on={adc && !adc.echo} onclick={() => engine(false)} disabled={!!busy}>clk_fast</button>
            <button class:on={adc?.echo} onclick={() => engine(true)} disabled={!!busy}>echo (DCM)</button>
          </div>
          <div class="row">
            <span class="lbl">sweep</span>
            <label><input type="number" min="20" max="5000" step="50" bind:value={sweepDwell} /> ms a phase</label>
            <button onclick={runSweep} disabled={!!busy}>Walk phases 0..4</button>
          </div>
          {#if sweep}
            <table class="grp">
              <thead><tr><th>phase</th><th>A</th><th>B</th><th>ADC_ERRORS</th></tr></thead>
              <tbody>
                {#each sweep.steps as s (s.phase)}
                  <tr><td>{s.phase} ({s.phase * 5} ns)</td><td class:ok={s.held[0] && !s.errors}>{s.held[0] ? 'held' : '—'}</td><td class:ok={s.held[1] && !s.errors}>{s.held[1] ? 'held' : '—'}</td><td class:bad={!!s.errors}>+{s.errors}</td></tr>
                {/each}
              </tbody>
            </table>
            {#if sweep.good}<p class="small">Good phases: A {maskList(sweep.good[0])}, B {maskList(sweep.good[1])}. The measured phase should sit inside a window ≥ 2 phases wide.</p>{/if}
          {/if}
        </section>
      {/if}

      <!-- registers -->
      <section class="regs">
        <h3>FPGA registers</h3>
        <div class="row wrap">
          <button onclick={readRegs} disabled={!!busy}>Read all</button>
          <span class="sp"></span>
          <label># <input type="number" min="0" max={dbg.FPGA_REGS - 1} bind:value={regIdx} /></label>
          <span class="muted">{core.reg_name(Math.max(0, Math.min(dbg.FPGA_REGS - 1, Math.round(regIdx || 0)))) || '—'}</span>
          <input class="mono val" bind:value={regVal} onkeydown={(e) => e.key === 'Enter' && writeReg()} />
          <button onclick={writeReg} disabled={!!busy}>Write</button>
        </div>
        <div class="regtable">
          <table>
            <thead><tr><th>#</th><th>name</th><th>hex</th><th>decimal</th></tr></thead>
            <tbody>
              {#each Array.from({ length: dbg.FPGA_REGS }, (_, i) => i) as i (i)}
                <tr class:sel={i === regIdx} onclick={() => pickReg(i)}>
                  <td>{i}</td>
                  <td class="muted">{core.reg_name(i)}</td>
                  <td class="mono">{regs?.[i] != null ? hex(regs[i]!) : '—'}</td>
                  <td class="mono">{regs?.[i] != null ? regs[i]!.toLocaleString() : ''}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
        <p class="muted small">Click a row to pick it. Range timings are in FPGA clk cycles (CLK_HZ).</p>
      </section>

      <!-- board -->
      {#if board}
        <section>
          <h3>Board (low level)</h3>
          <h4>LEDs</h4>
          <div class="row wrap">
            <label class="check"><input type="checkbox" checked={board.led_manual} onchange={(e) => bset('LEDs', bd.SET.LEDS, e.currentTarget.checked ? 0x100 | board!.leds : 0)} /> manual</label>
            {#each [0, 1, 2, 3, 4] as i (i)}
              <label class="check"><input type="checkbox" disabled={!board.led_manual} checked={!!((board.leds >> i) & 1)} onchange={(e) => setLed(i, e.currentTarget.checked)} /> LED{i}</label>
            {/each}
            <span class="dotf" class:on={bf(dbg.BFLAG.DFU)}>DFU button</span>
          </div>
          <h4>USB3 mux</h4>
          <div class="row wrap">
            <label>enable
              <select value={bf(dbg.BFLAG.USB3_EN_AUTO) ? 2 : bf(dbg.BFLAG.USB3_EN) ? 1 : 0} onchange={(e) => bset('USB3 mux', bd.SET.USB3_EN, +e.currentTarget.value)}>
                <option value={0}>off</option><option value={1}>on</option><option value={2}>auto (while attached)</option>
              </select>
            </label>
            <span class="dotf" class:on={bf(dbg.BFLAG.USB3_EN)}>on</span>
            <label>SEL
              <select value={bf(dbg.BFLAG.USB3_AUTO) ? 2 : bf(dbg.BFLAG.USB3_DIR) ? 1 : 0} onchange={(e) => bset('USB3 SEL', bd.SET.USB3_DIR, +e.currentTarget.value)}>
                <option value={0}>0 (RX2/TX2)</option><option value={1}>1 (RX1/TX1)</option><option value={2}>orientation</option>
              </select>
            </label>
            <span class="muted">= {bf(dbg.BFLAG.USB3_DIR) ? 1 : 0}</span>
          </div>
          <h4>FPGA configuration</h4>
          <div class="row wrap">
            <span class="dotf" class:on={bf(dbg.BFLAG.FPGA_DONE)}>DONE</span>
            <span class="dotf" class:on={bf(dbg.BFLAG.FPGA_INIT_B)}>INIT_B</span>
            <span class="dotf warn" class:on={bf(dbg.BFLAG.FPGA_PROG_HELD)}>PROGRAM_B held</span>
            <span class="muted mono">ID {hex(board.fpga_id)}</span>
          </div>
          {#if board.fpga_cfg_ms}<p class="muted small">Last configuration {board.fpga_cfg_ms} ms{board.fpga_cfg_bytes ? `, ${board.fpga_cfg_bytes.toLocaleString()} bytes from the H417` : ''}</p>{/if}
          <div class="row wrap">
            <button disabled={streaming || !!busy} onclick={() => bset('PROGRAM_B pulse', bd.SET.FPGA_PROGRAM, 2)}>Pulse PROGRAM_B</button>
            <button disabled={streaming || !!busy} onclick={() => bset('PROGRAM_B', bd.SET.FPGA_PROGRAM, bf(dbg.BFLAG.FPGA_PROG_HELD) ? 0 : 1)}>{bf(dbg.BFLAG.FPGA_PROG_HELD) ? 'Release PROGRAM_B' : 'Hold PROGRAM_B low'}</button>
          </div>
          {#if streaming}<p class="muted small">Stop the stream to reconfigure the FPGA (the board answers BUSY).</p>{/if}
          <h4>Screen</h4>
          <div class="row wrap">
            <select value={board.lcd_mode} onchange={(e) => bset('LCD', bd.SET.LCD, +e.currentTarget.value)}>
              <option value={0}>off</option><option value={1}>UI (menus)</option><option value={2}>colour bars</option>
            </select>
            <span class="lbl">remote</span>
            <button class="key" onclick={() => uiKey(dbg.KEY.UP)} aria-label="Up">▲</button>
            <button class="key" onclick={() => uiKey(dbg.KEY.DOWN)} aria-label="Down">▼</button>
            <button class="key" onclick={() => uiKey(dbg.KEY.LEFT)} aria-label="Left">◀</button>
            <button class="key" onclick={() => uiKey(dbg.KEY.RIGHT)} aria-label="Right">▶</button>
            <button class="key" onclick={() => uiKey(dbg.KEY.ENTER)}>Enter</button>
            <button class="key" onclick={() => uiKey(dbg.KEY.BACK)}>Back</button>
          </div>
          <label class="check" title="While this tab is open and no text field has the focus"><input type="checkbox" bind:checked={keys} /> Keyboard: arrows, Enter, Backspace / Esc drive the board's screen</label>
        </section>

        <!-- flash -->
        <section>
          <h3>SPI flash</h3>
          <p class="muted small">JEDEC {hex(board.flash_jedec, 6)}, {(board.flash_bytes / 1024).toFixed(0)} KB. The last 4 KB sector is the self-test's scratch; the running gateware and boot record are PROTECTED.</p>
          <div class="row wrap">
            <label>address <input class="mono val" bind:value={flAddr} /></label>
            <label>length <input class="mono val" bind:value={flLen} /></label>
            <button onclick={flashRead} disabled={!!busy || streaming}>Read</button>
            <button onclick={flashErase} disabled={!!busy || streaming}>Erase</button>
          </div>
          <div class="row wrap">
            <label class="file">
              <input type="file" onchange={pickFlashFile} />
              <span class="btn">{flFile ? `${flFile.name} (${flFile.bytes.length.toLocaleString()} B)` : 'Choose a file…'}</span>
            </label>
            <button onclick={flashWrite} disabled={!flFile || !!busy || streaming}>Erase, write, verify</button>
          </div>
          {#if streaming}<p class="muted small">Stop the stream first: the board answers BUSY while it streams.</p>{/if}
          {#if flData}
            <div class="row"><span class="muted">{flData.bytes.length.toLocaleString()} bytes from 0x{flData.addr.toString(16)}</span><span class="sp"></span><button class="ghost" onclick={flashSave}>Save…</button></div>
            <pre class="dump">{dbg.hexDump(flData.bytes, flData.addr, 256)}</pre>
          {/if}
        </section>
      {/if}

      <!-- raw -->
      <section>
        <h3>Raw command</h3>
        <div class="row wrap">
          <label>cmd <input class="mono short" bind:value={rawCmd} /></label>
          <label class="grow">payload <input class="mono" bind:value={rawPayload} placeholder="hex bytes, e.g. 1f 00 00 00" onkeydown={(e) => e.key === 'Enter' && sendRaw()} /></label>
          <button onclick={sendRaw} disabled={!!busy}>Send</button>
        </div>
        <p class="muted small">0x01 PING · 0x02 GET_INFO · 0x03 REG_READ · 0x12 GET_STATS · 0x22 GET_RANGE · 0x38 BOARD_STATUS · 0x3F UPDATE (see the protocol docs)</p>
        {#if rawOut}
          <p class:bad={rawOut.status !== 0}>{core.status_name(rawOut.status)} · {rawOut.payload.length} bytes · {rawOut.ms.toFixed(2)} ms</p>
          {#if rawOut.payload.length}<pre class="dump">{dbg.hexDump(rawOut.payload)}</pre>{/if}
        {/if}
      </section>

      <!-- log -->
      {@render logPane('', 'Nothing sent yet.')}
    </div>
    {/if}
  {/if}
</div>

<style>
  .debug {
    height: 100%;
    overflow-y: auto;
    padding: 10px;
    min-height: 0;
  }
  .empty {
    color: var(--muted);
    text-align: center;
    margin-top: 20vh;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-bottom: 10px;
    flex-wrap: wrap;
  }
  .sp {
    flex: 1;
  }
  .busy {
    font-size: 12px;
    color: var(--muted);
    display: inline-flex;
    gap: 6px;
    align-items: center;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(380px, 1fr));
    gap: 10px;
    align-items: start;
  }
  section {
    border: 1px solid var(--border);
    border-radius: var(--r);
    background: var(--panel);
    padding: 10px 12px;
    min-width: 0;
  }
  h3 {
    margin: 0 0 8px;
    font-size: 13px;
    display: flex;
    align-items: center;
    gap: 8px;
  }
  h4 {
    margin: 10px 0 4px;
    font-size: 12px;
    color: var(--muted);
    font-weight: 600;
  }
  p {
    margin: 4px 0;
    font-size: 12.5px;
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
  .ok {
    color: var(--ok);
  }
  .bad {
    color: var(--danger);
  }
  .danger-text {
    color: var(--danger);
  }
  .row {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 6px 0;
  }
  .row.wrap {
    flex-wrap: wrap;
  }
  .lbl {
    font-size: 11.5px;
    color: var(--muted);
    min-width: 48px;
  }
  label {
    font-size: 12px;
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }
  label.grow {
    flex: 1;
  }
  label.grow input {
    flex: 1;
    min-width: 0;
  }
  input[type='number'] {
    width: 64px;
  }
  select {
    width: auto;
  }
  label.check {
    white-space: nowrap;
  }
  input.val {
    width: 110px;
  }
  input.short {
    width: 56px;
  }
  table {
    border-collapse: collapse;
    width: 100%;
    font-size: 12px;
  }
  .kv th {
    text-align: left;
    font-weight: normal;
    color: var(--muted);
    padding: 1px 6px 1px 0;
    white-space: nowrap;
  }
  .kv td {
    padding: 1px 10px 1px 0;
    font-variant-numeric: tabular-nums;
  }
  .grp th,
  .regtable th {
    text-align: left;
    font-weight: 600;
    color: var(--muted);
    border-bottom: 1px solid var(--border);
    padding: 2px 6px 2px 0;
  }
  .grp td,
  .regtable td {
    padding: 2px 6px 2px 0;
    white-space: nowrap;
  }
  .regtable {
    max-height: 340px;
    overflow-y: auto;
  }
  .regtable tr {
    cursor: pointer;
  }
  .regtable tr:hover td {
    background: color-mix(in srgb, var(--accent) 8%, transparent);
  }
  .regtable tr.sel td {
    background: color-mix(in srgb, var(--accent) 16%, transparent);
  }
  .tap {
    min-width: 30px;
    padding: 2px 6px;
  }
  .tap.on,
  button.on {
    background: var(--ok);
    color: #fff;
    border-color: transparent;
  }
  .comp,
  .dotf {
    font-size: 11px;
    padding: 1px 6px;
    border-radius: var(--r-sm);
    border: 1px solid var(--border);
    color: var(--muted);
  }
  .comp.on {
    background: color-mix(in srgb, var(--warn) 25%, transparent);
    border-color: var(--warn);
    color: var(--text);
  }
  .dotf.on {
    background: color-mix(in srgb, var(--ok) 22%, transparent);
    border-color: var(--ok);
    color: var(--text);
  }
  .dotf.warn.on {
    background: color-mix(in srgb, var(--danger) 22%, transparent);
    border-color: var(--danger);
  }
  details {
    margin-top: 6px;
    font-size: 12px;
  }
  summary {
    cursor: pointer;
    color: var(--muted);
  }
  .form {
    display: grid;
    gap: 4px;
    margin: 6px 0;
  }
  .spark {
    width: 100%;
    height: 90px;
    display: block;
    margin-top: 8px;
    border: 1px solid var(--border);
    border-radius: var(--r-sm);
  }
  .legend {
    font-size: 11px;
    display: flex;
    gap: 10px;
    margin-top: 2px;
  }
  .l1 {
    color: var(--accent);
  }
  .l2 {
    color: var(--warn);
  }
  .key {
    padding: 2px 8px;
  }
  .file input {
    display: none;
  }
  .file .btn {
    border: 1px solid var(--border);
    border-radius: var(--r-sm);
    padding: 3px 8px;
    cursor: pointer;
    max-width: 220px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .dump {
    font-family: ui-monospace, 'SF Mono', Menlo, Consolas, monospace;
    font-size: 11px;
    max-height: 260px;
    overflow: auto;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: var(--r-sm);
    padding: 6px;
    margin: 6px 0 0;
    white-space: pre;
  }
  .log {
    max-height: 260px;
    overflow-y: auto;
    font-size: 11.5px;
    font-family: ui-monospace, 'SF Mono', Menlo, Consolas, monospace;
  }
  .log .line {
    position: relative;
    padding-right: 24px;
    user-select: text;
  }
  .log .copy {
    position: absolute;
    right: 0;
    top: -1px;
    padding: 0 4px;
    opacity: 0;
  }
  .log .line:hover .copy,
  .log .copy:focus-visible {
    opacity: 1;
  }
  .logsec h3 button + button {
    margin-left: 4px;
  }
  .logsec {
    grid-column: 1 / -1;
  }
  .bulog {
    margin-top: 10px;
  }
  .subtabs {
    display: flex;
    gap: 4px;
    margin-bottom: 10px;
  }
  .subtabs button {
    border-radius: var(--r-lg);
    padding: 3px 12px;
  }
  .subtabs button.on {
    background: var(--accent);
    color: #fff;
    border-color: transparent;
  }
  @media (max-width: 600px) {
    .grid {
      grid-template-columns: 1fr;
    }
  }
</style>
