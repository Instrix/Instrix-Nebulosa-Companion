// The Debug tab's commands: FPGA registers, the range switch's timing and
// forced comparators, the RS1473 ADC link, raw flash, round trips and raw
// NBP commands. Layouts are the Rust core's (debug.rs). The tab exists only
// in builds made with NEBULOSA_DEBUG=1 (see vite.config.ts).

import type { Device } from './device';
import { NbpError, core } from './nbp';

export const CMD = {
  PING: 0x01,
  REG_READ: 0x03,
  REG_WRITE: 0x04,
  RESET: 0x05,
  GET_STATS: 0x12,
  GET_RANGE: 0x22,
  SET_RANGE_TIMING: 0x23,
  SET_RANGE_FORCE: 0x24,
  FLASH_READ: 0x3b,
  FLASH_WRITE: 0x3c,
  FLASH_ERASE: 0x3d,
  FPGA_CONFIG: 0x3e,
} as const;

export const FPGA_REGS = 45;
export const REG = {
  ADC_CTRL: 31,
  ID: 34,
  RANGE_CFG: 40,
  CLK_HZ: 41,
  ADC_TIMING: 42,
  ADC_DELAY: 43,
  ADC_ERRORS: 44,
} as const;
export const FPGA_ID_CLK_HZ = 0x4e420006;
export const FPGA_ID_RS1473 = 0x4e42000a;
export const CLK_HZ_OLD = 108e6;

export const ADC_CTRL = { RECAL: 1 << 3, MANUAL: 1 << 4, PHASES: 0x7e0, ECHO: 1 << 11, HOLD: 1 << 2 } as const;
export const ADC_PHASES = 5;
export const FLASH_CHUNK = 256;
export const FLASH_SECTOR = 4096;

/** nbp_board_status_t.flags (board.rs flag) */
export const BFLAG = {
  HSE: 1 << 0,
  LSE: 1 << 1,
  RTC: 1 << 2,
  DFU: 1 << 3,
  PWR_ST: 1 << 5,
  USB3_EN: 1 << 8,
  USB3_DIR: 1 << 9,
  FPGA_DONE: 1 << 10,
  FPGA_INIT_B: 1 << 11,
  FPGA_PROG_HELD: 1 << 12,
  LCD: 1 << 15,
  SWD: 1 << 16,
  USB3_AUTO: 1 << 17,
  USB3_EN_AUTO: 1 << 18,
} as const;

export const KEY = { UP: 1, DOWN: 2, LEFT: 3, RIGHT: 4, ENTER: 5, BACK: 6 } as const;

/** full scale per range (0: the least sensitive) */
export const RANGE_FS = ['2 A', '80 mA', '3.2 mA', '127 µA', '5.1 µA', '200 nA'];
/** comparator bits, low first */
export const COMP_NAMES = ['UP A', 'DOWN A', 'UP B', 'DOWN B', 'UP C', 'DOWN C'];

/** debug.rs RangeDecoded */
export interface RangeDecoded {
  range: number;
  switching: boolean;
  taps: number;
  comps: number;
  automatic: boolean;
  forced: boolean;
  manual: number;
  min: number;
  max: number;
  up_filt: number;
  down_filt: number;
  overlap: number;
  hold: number;
  settle: number;
  align: boolean;
  dirty_up: boolean;
  dirty_down: boolean;
  /** the raw comparator pins (RANGE_STATUS 29:24) */
  pins: number;
}

export interface AdcGroup {
  phase: number;
  locked: boolean;
  tmap: number;
  eye: number;
  delay: number;
  dcm_locked: boolean;
  echo_map: number;
  clean: number;
}

/** debug.rs AdcLink */
export interface AdcLink {
  ctrl: number;
  timing: number;
  delay: number;
  errors: number;
  manual: boolean;
  echo: boolean;
  hold: boolean;
  source: number;
  groups: [AdcGroup, AdcGroup];
  locked: boolean;
}

export async function regRead(d: Device, i: number): Promise<number> {
  const v = core.parse_reg(await d.cmd(CMD.REG_READ, core.enc_reg(i, 0)));
  if (v === undefined) throw new Error(`register ${i}: short response`);
  return v >>> 0;
}

export async function regWrite(d: Device, i: number, v: number) {
  await d.cmd(CMD.REG_WRITE, core.enc_reg(i, v >>> 0));
}

/** read-modify-write: clears then sets bits, returns the new value */
export async function regUpdate(d: Device, i: number, clear: number, set: number): Promise<number> {
  const v = ((((await regRead(d, i)) & ~clear) | set) >>> 0) as number;
  await regWrite(d, i, v);
  return v;
}

/** every register; null where a read failed */
export async function readAll(d: Device): Promise<(number | null)[]> {
  const out: (number | null)[] = [];
  for (let i = 0; i < FPGA_REGS; i++) out.push(await regRead(d, i).catch(() => null));
  return out;
}

export async function range(d: Device): Promise<{ raw: { status: number; count: number; ctrl: number; timing: number }; dec: RangeDecoded } | null> {
  const r = core.parse_range(await d.cmd(CMD.GET_RANGE));
  if (!r) return null;
  const cfg = await regRead(d, REG.RANGE_CFG).catch(() => 0);
  return { raw: r, dec: core.decode_range(r.status, r.ctrl, r.timing, cfg) };
}

export async function setRangeTiming(d: Device, t: { up_filt: number; down_filt: number; overlap: number; hold: number; settle: number; align: boolean; dirty_up: boolean; dirty_down: boolean }) {
  await d.cmd(CMD.SET_RANGE_TIMING, core.enc_range_timing(t.up_filt, t.down_filt, t.overlap, t.hold));
  await regWrite(d, REG.RANGE_CFG, core.enc_range_cfg(t.settle, t.align, t.dirty_up, t.dirty_down));
}

export async function setRangeForce(d: Device, enable: boolean, bits: number) {
  await d.cmd(CMD.SET_RANGE_FORCE, core.enc_range_force(enable, bits));
}

/** the FPGA's fabric clock (Hz): the unit of the range timing */
export async function clkHz(d: Device, fpgaId: number): Promise<number> {
  if (fpgaId < FPGA_ID_CLK_HZ) return CLK_HZ_OLD;
  const v = await regRead(d, REG.CLK_HZ).catch(() => 0);
  return v || CLK_HZ_OLD;
}

// ---- RS1473 ADC link ----------------------------------------------------------------

export async function adcLink(d: Device): Promise<AdcLink> {
  const c = await regRead(d, REG.ADC_CTRL);
  const t = await regRead(d, REG.ADC_TIMING);
  const dl = await regRead(d, REG.ADC_DELAY);
  const e = await regRead(d, REG.ADC_ERRORS);
  return core.decode_adc(c, t, dl, e);
}

/** RECAL edge (0 -> 1 -> 0): the gateware measures the eye again */
export async function adcRecal(d: Device) {
  await regUpdate(d, REG.ADC_CTRL, ADC_CTRL.RECAL, 0);
  await regUpdate(d, REG.ADC_CTRL, 0, ADC_CTRL.RECAL);
  await regUpdate(d, REG.ADC_CTRL, ADC_CTRL.RECAL, 0);
}

/** polls until both groups lock (and both DCMs, echo engine); ms it took, or null on timeout */
export async function adcWaitLock(d: Device, timeoutMs: number): Promise<number | null> {
  const t0 = performance.now();
  for (;;) {
    const l = await adcLink(d);
    const el = performance.now() - t0;
    if (l.locked) return el;
    if (el >= timeoutMs) return null;
    await sleep(2);
  }
}

/** fixed sample phases (null: measured, automatic) */
export async function adcPhases(d: Device, p: [number, number] | null) {
  if (p) await regUpdate(d, REG.ADC_CTRL, ADC_CTRL.MANUAL | ADC_CTRL.PHASES, ADC_CTRL.MANUAL | core.adc_phases(p[0], p[1]));
  else await regUpdate(d, REG.ADC_CTRL, ADC_CTRL.MANUAL | ADC_CTRL.PHASES, 0);
}

export async function adcEngine(d: Device, echo: boolean) {
  await regUpdate(d, REG.ADC_CTRL, ADC_CTRL.ECHO, echo ? ADC_CTRL.ECHO : 0);
}

export interface SweepStep {
  phase: number;
  held: [boolean, boolean];
  errors: number;
}

/** walks the phase 0..4 (dwell ms each): where each group holds the lock; then automatic again */
export async function adcSweep(d: Device, dwellMs: number, onStep: (s: SweepStep) => void): Promise<{ good: [number, number] }> {
  const good: [number, number] = [0, 0];
  try {
    for (let p = 0; p < ADC_PHASES; p++) {
      await adcPhases(d, [p, p]);
      await sleep(5);
      const l0 = await adcLink(d);
      const held: [boolean, boolean] = [true, true];
      const t0 = performance.now();
      let l = l0;
      do {
        l = await adcLink(d);
        for (let k = 0; k < 2; k++) held[k] = held[k] && l.groups[k].locked && l.groups[k].phase === p;
        await sleep(2);
      } while (performance.now() - t0 < dwellMs);
      const errors = (l.errors - l0.errors) >>> 0;
      // a phase that keeps the lock while losing frames is still bad (ADC_ERRORS counts both groups)
      for (let k = 0; k < 2; k++) if (held[k] && errors === 0) good[k] |= 1 << p;
      onStep({ phase: p, held, errors });
    }
  } finally {
    await adcPhases(d, null);
    await adcRecal(d);
    await adcWaitLock(d, 50);
  }
  return { good };
}

/** " x [.] . ." : x = a line changes at that phase, [ ] = the phase in use */
export function mapStr(map: number, phase: number): string {
  let s = '';
  for (let k = 0; k < ADC_PHASES; k++) {
    const ch = (map >> k) & 1 ? 'x' : '·';
    s += k === phase ? `[${ch}]` : ` ${ch} `;
  }
  return s;
}

// ---- flash ------------------------------------------------------------------------

export async function flashRead(d: Device, addr: number, len: number, onProgress?: (done: number) => void): Promise<Uint8Array> {
  const out = new Uint8Array(len);
  for (let o = 0; o < len; o += FLASH_CHUNK) {
    const n = Math.min(FLASH_CHUNK, len - o);
    const r = core.parse_flash_read(await d.cmd(CMD.FLASH_READ, core.enc_flash(addr + o, n, new Uint8Array())));
    if (!r || r.length !== n) throw new Error(`flash read at 0x${(addr + o).toString(16)}: short response`);
    out.set(r, o);
    onProgress?.(o + n);
  }
  return out;
}

/** erases the 4 KB sectors over [addr, addr + len) (addr 4 KB aligned) */
export async function flashErase(d: Device, addr: number, len: number) {
  const n = Math.ceil(len / FLASH_SECTOR) * FLASH_SECTOR;
  await d.cmd(CMD.FLASH_ERASE, core.enc_flash(addr, n, new Uint8Array()), 20000);
}

/** writes page by page (erase first); addr 256-byte aligned keeps each write inside one page */
export async function flashWrite(d: Device, addr: number, data: Uint8Array, onProgress?: (done: number) => void) {
  for (let o = 0; o < data.length; ) {
    const a = addr + o;
    const n = Math.min(FLASH_CHUNK - (a % FLASH_CHUNK), data.length - o);
    await d.cmd(CMD.FLASH_WRITE, core.enc_flash(a, n, data.subarray(o, o + n)));
    o += n;
    onProgress?.(o);
  }
}

// ---- link -----------------------------------------------------------------------------

/** PING round trips of `size` bytes: [min, mean, max] ms, from the app (includes its own IPC) */
export async function ping(d: Device, size: number, count: number): Promise<{ min: number; mean: number; max: number; bad: number }> {
  const p = new Uint8Array(size);
  for (let i = 0; i < size; i++) p[i] = (i * 31 + 7) & 0xff;
  let min = Infinity,
    max = 0,
    sum = 0,
    bad = 0;
  for (let k = 0; k < count; k++) {
    const t0 = performance.now();
    const r = await d.cmd(CMD.PING, p);
    const dt = performance.now() - t0;
    if (r.length !== size || r.some((b, i) => b !== p[i])) bad++;
    min = Math.min(min, dt);
    max = Math.max(max, dt);
    sum += dt;
  }
  return { min, mean: sum / count, max, bad };
}

/** any command: its status and payload, whatever the status */
export async function raw(d: Device, cmd: number, payload: Uint8Array, timeoutMs = 2000): Promise<{ status: number; payload: Uint8Array; ms: number }> {
  const t0 = performance.now();
  const r = await d.link.request(cmd, payload, timeoutMs);
  return { status: r.status, payload: r.payload, ms: performance.now() - t0 };
}

export function parseHex(s: string): Uint8Array {
  const t = s.replace(/0x/gi, '').replace(/[\s,:]+/g, '');
  if (t.length % 2 || /[^0-9a-f]/i.test(t)) throw new Error('hex bytes, e.g. "22 00 00 00"');
  const out = new Uint8Array(t.length / 2);
  for (let i = 0; i < out.length; i++) out[i] = parseInt(t.slice(2 * i, 2 * i + 2), 16);
  return out;
}

/** a hex dump, 16 bytes a line, with the address and ASCII */
export function hexDump(b: Uint8Array, base = 0, maxLines = 4096): string {
  const lines: string[] = [];
  for (let o = 0; o < b.length && lines.length < maxLines; o += 16) {
    const row = b.subarray(o, o + 16);
    const hx = Array.from(row, (x) => x.toString(16).padStart(2, '0')).join(' ');
    const asc = Array.from(row, (x) => (x >= 32 && x < 127 ? String.fromCharCode(x) : '.')).join('');
    lines.push(`${(base + o).toString(16).padStart(6, '0')}  ${hx.padEnd(47)}  ${asc}`);
  }
  if (b.length > maxLines * 16) lines.push(`… ${b.length - maxLines * 16} more bytes`);
  return lines.join('\n');
}

export function statusText(e: unknown): string {
  if (e instanceof NbpError) return e.message;
  return String(e instanceof Error ? e.message : e);
}

export const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
