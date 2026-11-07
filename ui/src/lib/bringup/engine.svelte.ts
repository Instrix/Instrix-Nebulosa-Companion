// The AFE bring-up (Instrix-Nebulosa Design/afe1-x10-bringup.md; the
// Nebulosa Debugger's src/gui/gui_bringup.cpp): the bench instruments, the
// interlocks, measurements on a tap through the core's probe (raw ADC codes
// of one range), the sequences of §5-§8, and the step-by-step playbook.
// Section numbers (§n) are the procedure's.
//
// Safety, as in the debugger: ALL OFF (the load's input and CH1) always
// works, also when the Nebulosa's link is lost and when the tab closes; the
// load can't be set above 1.1 × FS of the tap that is on, and a tap can't be
// stepped down past what the load draws; taps go down one at a time.

import type { Device } from '../device';
import { core } from '../nbp';
import * as dbg from '../debug';
import * as afe from './afe';
import { Load, Psu, simBench } from './instruments';

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

export interface Row {
  test: string;
  /** 1..6 */
  tap: number;
  /** reference current, A (NaN: none) */
  iRef: number;
  /** offset-corrected mean code, rms noise in codes */
  code: number;
  sd: number;
  n: number;
  /** fraction of samples flagged DIRTY */
  dirty: number;
  /** nominal reading, A */
  iNeb: number;
  /** (iNeb - iRef) / FS, % (or what the test says) */
  err: number;
  note: string;
}

export interface Meas {
  ok: boolean;
  mean: number;
  sd: number;
  n: number;
  dirty: number;
  range: number;
  why: string;
}

export type RefKind = 'load' | 'box' | 'typed';

export type StepKind =
  | 'check' // auto: Nebulosa connected and streaming, instruments open
  | 'timing' // auto: 0.5 µs make-before-break overlap
  | 'alloff' // auto: load input and CH1 off
  | 'tap' // auto: manual tap (down one at a time)
  | 'psu' // auto: CH1 set V / limit, on or off
  | 'load' // auto: KEL102 CC setpoint, input on or off
  | 'zero' // auto: the tap's zero with the loop open
  | 'meas' // auto: expected vs read
  | 'info' // operator: do what the text says, then Done
  | 'box' // operator: dial the decade box, enter what was dialled
  | 'dmm'; // operator: read the DMM, enter the value

export type StepState = 'pending' | 'running' | 'pass' | 'fail' | 'done' | 'skipped';

export interface Step {
  kind: StepKind;
  title: string;
  text: string;
  /** range index 0..5 */
  tap: number;
  /** psu: CH1 volts / limit; load: amps */
  v: number;
  a: number;
  on: boolean;
  /** box: what to dial */
  boxOhm: number;
  /** meas: the current the step was built for */
  iSet: number;
  ref: 'load' | 'box';
  /** dmm: the expected window */
  lo: number;
  hi: number;
  unit: string;
  needLoad: boolean;
  needPsu: boolean;
  state: StepState;
  result: string;
  expect: number;
  read: number;
  errFs: number;
  tol: number;
}

export const operatorStep = (k: StepKind) => k === 'info' || k === 'box' || k === 'dmm';

export const PLAYBOOKS = [
  'Tap identification (DMM, no current)',
  'Every tap, 1 → 6',
  'Tap 1 (2 A, load)',
  'Tap 2 (200 mA, load)',
  'Tap 3 (20 mA, box)',
  'Tap 4 (2 mA, box)',
  'Tap 5 (200 µA, box)',
  'Tap 6 (20 µA, box)',
];

const WIRE_LOAD =
  'Load mode: CH1 (+) → I+, I− → KEL102 (+), KEL102 (−) → CH1 (−). Short heavy leads (2 A). CH1 floating: nothing to the SPD’s earth terminal. Remove any short across I+ / I−.';
const WIRE_BOX =
  'Box mode: CH1 (+) → I+, I− → decade box → CH1 (−). KEL102 input across CH1 (+) / (−) as the voltmeter (its input stays OFF). Remove any short across I+ / I−.';

function mk(kind: StepKind, title: string, text = ''): Step {
  return { kind, title, text, tap: -1, v: 0, a: 0, on: false, boxOhm: 0, iSet: NaN, ref: 'load', lo: NaN, hi: NaN, unit: '', needLoad: false, needPsu: false, state: 'pending', result: '', expect: NaN, read: NaN, errFs: NaN, tol: NaN };
}

/** what the bring-up needs from the app */
export interface Host {
  dev(): Device | null;
  streaming(): boolean;
  /** starts the stream with the ADC as the source */
  startAdc(): Promise<void>;
  log(text: string, bad?: boolean): void;
  /** the demo device: the simulated bench's loop current as its ADC codes on `range` (null: its own data) */
  demoBench?(b: { range: number; code: number; pins: number } | null): void;
}

class Aborted extends Error {
  constructor(why: string) {
    super(why);
  }
}

export class Bringup {
  readonly psu = new Psu();
  readonly load = new Load();

  // instruments
  psuPath = $state('/dev/usbtmc0');
  loadPath = $state('/dev/ttyACM0');
  loadBaud = $state(115200);
  psuIdn = $state('');
  loadIdn = $state('');
  psuOpen = $state(false);
  loadOpen = $state(false);
  psuV = $state([NaN, NaN]);
  psuI = $state([NaN, NaN]);
  psuOut = $state([false, false, false]);
  loadV = $state(NaN);
  loadI = $state(NaN);
  loadIn = $state(false);
  /** the CC setpoint sent (A), for the interlock */
  loadSet = $state(0);

  // bench controls
  ch = $state([
    { v: 5.0, a: 0.05 },
    { v: 12.0, a: 1.0 },
  ]);
  loadCcMa = $state(0);
  refKind = $state<RefKind>('load');
  boxOhm = $state(10000);
  boxV = $state(10);
  typedUa = $state(0);
  measMs = $state(500);
  chainMap = $state<afe.ChainMap>('x25');
  overrideIl = $state(false);
  linTap = $state(1);
  linV = $state(5);
  cmpTap = $state(1);
  ratioV = $state(10);

  // the Nebulosa
  /** its manual range, -1: automatic / unknown */
  manualRange = $state(-1);
  rangeDec = $state<dbg.RangeDecoded | null>(null);
  clkHz = $state(1e8);

  // results
  rows = $state<Row[]>([]);
  zero = $state<number[]>([NaN, NaN, NaN, NaN, NaN, NaN]);
  zeroSd = $state<number[]>([NaN, NaN, NaN, NaN, NaN, NaN]);
  last = $state<Meas | null>(null);

  // sequences
  status = $state('idle');
  busy = $state(false);
  prompt = $state<{ text: string; resolve: (go: boolean) => void } | null>(null);
  private abort = false;

  // playbook
  steps = $state<Step[]>([]);
  cur = $state(0);
  executing = $state(false);
  boxActual = NaN;
  pbName = $state('');
  pbKind = $state(1);
  pbBoxV = $state(10);
  pbBoxTol = $state(1);
  pbGainTol = $state(1);
  pbAuto = $state(true);

  private unsub: () => void;
  private timer: ReturnType<typeof setInterval>;

  constructor(private host: Host) {
    this.unsub = simBench.onChange(() => this.syncDemo());
    this.timer = setInterval(() => void this.poll(), 500);
  }

  /** leaving: everything off, the demo back to its own data */
  async destroy() {
    clearInterval(this.timer);
    this.unsub();
    this.abortNow();
    await this.everythingOff().catch(() => undefined);
    this.host.demoBench?.(null);
  }

  private log(s: string, bad = false) {
    this.host.log(`bring-up: ${s}`, bad);
  }

  // ---- instruments -------------------------------------------------------------------

  async openPsu() {
    try {
      await this.psu.connect('psu', this.psuPath.trim(), 0);
      await this.psu.independent();
      await this.psu.output(1, false);
      this.psuOut[0] = false;
      const id = await this.psu.identify().catch(() => '');
      this.psuIdn = id || '(no answer to *IDN?)';
      this.psuOpen = true;
      this.log(`PSU ${this.psuIdn}: independent mode, CH1 off`);
    } catch (e) {
      this.psuOpen = false;
      this.log(`PSU: ${String(e instanceof Error ? e.message : e)}`, true);
    }
  }

  async openLoad() {
    try {
      await this.load.connect('load', this.loadPath.trim(), this.loadBaud);
      await this.load.input(false);
      this.loadIn = false;
      const id = await this.load.identify().catch(() => '');
      this.loadIdn = id || '(no answer to *IDN?: baud rate? the load’s menu sets it)';
      this.loadOpen = true;
      this.log(`load ${this.loadIdn}: input off`);
    } catch (e) {
      this.loadOpen = false;
      this.log(`load: ${String(e instanceof Error ? e.message : e)}`, true);
    }
  }

  async closeInstruments() {
    await this.everythingOff().catch(() => undefined);
    await this.psu.disconnect();
    await this.load.disconnect();
    this.psuOpen = this.loadOpen = false;
    this.syncDemo();
  }

  /** the load's input and CH1 off (not through a sequence: it always works) */
  async everythingOff(): Promise<boolean> {
    let ok = true;
    if (this.load.open) ok = (await this.load.input(false).then(() => true, () => false)) && ok;
    if (this.psu.open) ok = (await this.psu.output(1, false).then(() => true, () => false)) && ok;
    this.loadIn = false;
    this.psuOut[0] = false;
    return ok;
  }

  /** ALL OFF: stops what runs, then the load and CH1 off */
  async allOff(why = 'ALL OFF') {
    this.abortNow();
    const ok = await this.everythingOff();
    this.log(`${why}${ok ? '' : ' (an instrument did not answer: check it)'}`, !ok);
  }

  abortNow() {
    this.abort = true;
    const p = this.prompt;
    this.prompt = null;
    p?.resolve(false);
  }

  /** the link to the Nebulosa went away: stop, and off */
  linkLost() {
    if (this.busy || this.loadIn || this.psuOut[0]) void this.allOff('Nebulosa link lost: ALL OFF');
  }

  private polling = false;
  async poll() {
    if (this.busy || this.polling) return;
    this.polling = true;
    try {
      if (this.psu.open)
        for (const ch of [1, 2]) {
          const m = await this.psu.measure(ch).catch(() => null);
          if (m) {
            this.psuV[ch - 1] = m.v;
            this.psuI[ch - 1] = m.a;
          }
        }
      if (this.load.open) {
        const m = await this.load.measure().catch(() => null);
        if (m) {
          this.loadV = m.v;
          this.loadI = m.a;
        }
      }
      await this.readRange();
    } finally {
      this.polling = false;
    }
  }

  async setPsu(ch: number) {
    const c = this.ch[ch - 1];
    await this.psu.set(ch, Math.min(32, Math.max(0, c.v)), Math.min(3.2, Math.max(0, c.a))).catch((e) => this.log(`PSU CH${ch} set: ${e}`, true));
  }

  async togglePsu(ch: number) {
    const to = !this.psuOut[ch - 1];
    if (ch === 1 && to && this.manualRange < 0 && !this.overrideIl) return this.log('CH1 stays off: take a tap (manual) first', true);
    await this.psu.output(ch, to).then(
      () => (this.psuOut[ch - 1] = to),
      (e) => this.log(`PSU CH${ch}: ${e}`, true),
    );
  }

  /** the load in CC, interlocked to the tap that is on */
  async loadCc(amps: number, on: boolean): Promise<boolean> {
    const r = this.manualRange;
    if (on && !this.overrideIl && (r < 0 || amps > afe.ilLimit(r) + 1e-9)) {
      this.log(`interlock: ${afe.siAmps(amps)} on tap ${r + 1} refused (limit ${afe.siAmps(afe.ilLimit(r))})`, true);
      return false;
    }
    try {
      await this.load.mode('CC');
      await this.load.setCurrent(amps);
      if (on) await this.load.input(true);
      this.loadSet = amps;
      if (on) this.loadIn = true;
      return true;
    } catch (e) {
      this.log(`load: ${e}`, true);
      return false;
    }
  }

  async toggleLoad() {
    if (this.loadIn) {
      await this.load.input(false).then(
        () => (this.loadIn = false),
        (e) => this.log(`load: ${e}`, true),
      );
    } else await this.loadCc(this.loadCcMa * 1e-3, true);
  }

  /** the load's current, averaged (KEL102: 0.1 mA resolution on its 3 A range) */
  async loadAmps(n = 3): Promise<number> {
    let s = 0,
      k = 0;
    for (let i = 0; i < n; i++) {
      const m = await this.load.measure().catch(() => null);
      if (m && Number.isFinite(m.a)) {
        s += m.a;
        k++;
        this.loadV = m.v;
        this.loadI = m.a;
      }
    }
    return k ? s / k : NaN;
  }

  // ---- the Nebulosa ----------------------------------------------------------------

  async readRange() {
    const d = this.host.dev();
    if (!d) {
      this.rangeDec = null;
      return;
    }
    const r = await dbg.range(d).catch(() => null);
    this.rangeDec = r?.dec ?? null;
    if (r && !this.busy) this.manualRange = r.dec.automatic ? -1 : r.dec.manual;
  }

  /** Steps to tap index `to` (0..5): down one tap at a time (TR-0002 rule 2), up in one jump; refused when the load's setpoint doesn't fit a range on the way. */
  async gotoRange(to: number) {
    const d = this.host.dev();
    if (!d) throw new Aborted('Nebulosa not connected');
    const r = await dbg.range(d).catch(() => null);
    let cur = r && !r.dec.automatic ? r.dec.manual : -1;
    if (cur < 0) {
      // automatic or unknown: tap 1 first, the safe one
      await d.setRange(false, 0, 0, 5);
      cur = 0;
      await sleep(20);
    }
    while (cur !== to) {
      const next = to < cur ? to : cur + 1;
      if (this.loadIn && !this.overrideIl && this.loadSet > afe.ilLimit(next) + 1e-9)
        throw new Aborted(`load at ${afe.siAmps(this.loadSet)} does not fit tap ${next + 1} (${afe.FS_LABEL[next]})`);
      await d.setRange(false, next, 0, 5);
      cur = next;
      await sleep(30);
    }
    this.manualRange = cur;
    simBench.rTap = afe.R_CUM[cur] * 1.25;
    this.syncDemo();
  }

  async goTap(i: number) {
    if (this.busy) return;
    await this.gotoRange(i).catch((e) => this.log(`tap ${i + 1}: ${e instanceof Error ? e.message : e}`, true));
    await this.readRange();
  }

  /** the bring-up's switch timing: 0.5 µs make-before-break overlap */
  async setOverlap(): Promise<string> {
    const d = this.host.dev();
    if (!d) throw new Aborted('not connected');
    const info = await d.info().catch(() => null);
    this.clkHz = await dbg.clkHz(d, info?.fpga_id ?? 0);
    const r = await dbg.range(d);
    if (!r) throw new Aborted('GET_RANGE failed');
    const ov = Math.max(0, Math.min(255, Math.round(0.5e-6 * this.clkHz) - 1));
    const t = r.dec;
    await d.cmd(dbg.CMD.SET_RANGE_TIMING, core.enc_range_timing(t.up_filt, t.down_filt, ov, t.hold));
    return `overlap ${ov + 1} cycles at ${(this.clkHz / 1e6).toFixed(0)} MHz = ${((ov + 1) / this.clkHz * 1e6).toFixed(2)} µs`;
  }

  /** ms of samples on range r: mean and rms of the clean ones (raw codes) */
  async measure(r: number, ms: number): Promise<Meas> {
    const m: Meas = { ok: false, mean: 0, sd: 0, n: 0, dirty: 0, range: r, why: '' };
    const d = this.host.dev();
    if (!d || !this.host.streaming()) {
      m.why = 'stream not running (Start, source ADC)';
      return m;
    }
    await sleep(20); // blocks in flight from before
    await d.link.probeArm(r);
    await sleep(ms);
    const p = await d.link.probeTake();
    if (!p || p.n + p.dirty === 0) {
      m.why = 'no ADC samples in that range (source ADC? range?)';
      return m;
    }
    m.n = p.n;
    m.dirty = p.dirty;
    m.mean = p.mean;
    m.sd = p.sd;
    m.ok = true;
    if (p.all_dirty) m.why = 'every sample DIRTY (comparator asserted?)';
    return m;
  }

  zeroOf(r: number) {
    return Number.isNaN(this.zero[r]) ? 0 : this.zero[r];
  }

  private makeRow(test: string, r: number, m: Meas, iRef: number, note = ''): Row {
    const code = m.mean - this.zeroOf(r);
    const iNeb = afe.codeToAmps(code, r, this.chainMap);
    return {
      test,
      tap: r + 1,
      iRef,
      code,
      sd: m.sd,
      n: m.n,
      dirty: m.n + m.dirty ? m.dirty / (m.n + m.dirty) : 0,
      iNeb,
      err: Number.isNaN(iRef) ? NaN : ((iNeb - iRef) / afe.FS_A[r]) * 100,
      note: note ? note + (m.why ? `; ${m.why}` : '') : m.why,
    };
  }

  private addRow(w: Row) {
    this.rows.push(w);
  }

  private checkAbort() {
    if (this.abort) throw new Aborted('aborted');
    if (!this.host.dev()) throw new Aborted('Nebulosa link lost');
  }

  /** waits for the operator; false if aborted */
  private ask(text: string): Promise<boolean> {
    return new Promise((resolve) => (this.prompt = { text, resolve }));
  }

  promptDone(go: boolean) {
    const p = this.prompt;
    this.prompt = null;
    if (!go) this.abort = true;
    p?.resolve(go);
  }

  /** the measure button: what the tap reads against the reference picked */
  async measureNow() {
    if (this.busy) return;
    const r = this.manualRange;
    if (r < 0) return this.log('measure: take a tap (manual) first', true);
    this.busy = true;
    try {
      let ref = NaN;
      if (this.refKind === 'load' && this.load.open) ref = await this.loadAmps(2);
      else if (this.refKind === 'box') {
        const i = this.boxOhm > 0 ? this.boxV / this.boxOhm : NaN;
        ref = i - afe.burdenV(i, r) / (this.boxOhm > 0 ? this.boxOhm : 1);
      } else if (this.refKind === 'typed') ref = this.typedUa * 1e-6;
      const m = await this.measure(r, this.measMs);
      this.last = m;
      if (!m.ok) return this.log(`measure: ${m.why}`, true);
      this.addRow(this.makeRow('measure', r, m, ref));
    } finally {
      this.busy = false;
    }
  }

  // ---- sequences ------------------------------------------------------------------------

  /** runs a sequence; everything off if it stops early, the load off when it ends */
  async runSequence(name: string, fn: () => Promise<void>) {
    if (this.busy) return;
    this.busy = true;
    this.abort = false;
    this.status = `${name}: running`;
    this.log(`${name}: start`);
    try {
      await fn();
      if (this.load.open) await this.load.input(false).catch(() => undefined);
      this.loadIn = false;
      this.status = `${name}: done`;
      this.log(`${name}: done`);
    } catch (e) {
      await this.everythingOff();
      this.status = `${name}: stopped (${e instanceof Error ? e.message : e}), load and CH1 off`;
      this.log(this.status, true);
    } finally {
      this.prompt = null;
      this.busy = false;
    }
  }

  private async psuOn(v: number, a: number) {
    await this.psu.set(1, v, a);
    await this.psu.output(1, true);
    this.psuOut[0] = true;
  }

  private async mustLoad(amps: number) {
    if (!(await this.loadCc(amps, true))) throw new Aborted('the load refused (interlock) or did not answer');
  }

  /** §5: zero and noise on every tap, nothing connected or the loop open */
  async seqZero() {
    for (let r = 0; r < afe.TAPS; r++) {
      this.checkAbort();
      await this.gotoRange(r);
      await sleep(100);
      const m = await this.measure(r, this.measMs);
      if (!m.ok) throw new Aborted(m.why);
      this.zero[r] = m.mean;
      this.zeroSd[r] = m.sd;
      const fsCode = afe.ampsToCode(afe.FS_A[r], r, this.chainMap);
      const iNeb = afe.codeToAmps(m.mean, r, this.chainMap);
      const off = (iNeb / afe.FS_A[r]) * 100;
      this.addRow({
        test: 'zero',
        tap: r + 1,
        iRef: NaN,
        code: m.mean,
        sd: m.sd,
        n: m.n,
        dirty: m.n + m.dirty ? m.dirty / (m.n + m.dirty) : 0,
        iNeb,
        err: off,
        note: `offset ${off.toFixed(3)} % FS, FS/rms ${m.sd > 0 ? (fsCode / m.sd).toFixed(0) : '—'} (full band)${m.why ? `; ${m.why}` : ''}`,
      });
      this.status = `zero: tap ${r + 1} ${m.mean.toFixed(1)} codes, ${m.sd.toFixed(2)} rms`;
    }
  }

  /** §6: linearity on taps 1-3 with the load in CC (CH1 → I+, I− → load) */
  async seqLinearity(tap: number, vSupply: number) {
    const r = tap - 1;
    const pts = [0.0, 0.1, 0.25, 0.5, 0.75, 0.85];
    const iMax = 0.85 * afe.FS_A[r];
    await this.gotoRange(r);
    await this.psuOn(vSupply, Math.min(3.2, iMax * 1.15 + 0.01));
    await this.mustLoad(0);
    const xs: number[] = [],
      ys: number[] = [];
    for (const p of pts) {
      this.checkAbort();
      await this.mustLoad(p * afe.FS_A[r]);
      await sleep(400);
      const ref = await this.loadAmps(3);
      const m = await this.measure(r, this.measMs);
      const ref2 = await this.loadAmps(2);
      if (!m.ok) throw new Aborted(m.why);
      const iref = (ref * 3 + ref2 * 2) / 5;
      const w = this.makeRow(`lin tap${tap}`, r, m, iref, `${(p * 100).toFixed(0)} % FS`);
      this.addRow(w);
      xs.push(iref);
      ys.push(w.code);
      this.status = `linearity tap ${tap}: ${(p * 100).toFixed(0)} % FS: ref ${afe.siAmps(iref)}, read ${afe.siAmps(w.iNeb)}`;
    }
    // least squares code = g × I + o
    const n = xs.length;
    let sx = 0,
      sy = 0,
      sxx = 0,
      sxy = 0;
    for (let i = 0; i < n; i++) {
      sx += xs[i];
      sy += ys[i];
      sxx += xs[i] * xs[i];
      sxy += xs[i] * ys[i];
    }
    const den = n * sxx - sx * sx;
    if (den <= 0) return;
    const g = (n * sxy - sx * sy) / den,
      o = (sy - g * sx) / n;
    const gNom = afe.ampsToCode(1, r, this.chainMap);
    let inl = 0;
    for (let i = 0; i < n; i++) inl = Math.max(inl, Math.abs(ys[i] - (g * xs[i] + o)));
    const fsCode = gNom * afe.FS_A[r];
    const note = `gain ${((g / gNom - 1) * 100).toFixed(3)} % vs nominal, offset ${((o / fsCode) * 100).toFixed(4)} % FS, INL ${((inl / fsCode) * 100).toFixed(4)} % FS (ref: KEL102, ±${afe.siAmps(0.0005 * afe.FS_A[r] + 0.0015)})`;
    this.addRow({ test: `fit tap${tap}`, tap, iRef: NaN, code: NaN, sd: NaN, n: 0, dirty: 0, iNeb: NaN, err: NaN, note });
    this.log(note);
  }

  /**
   * §7: ladder ratio transfer. The same current, 80 % of tap k+1's FS, read
   * on tap k (8 % FS) and k+1; the ratio of the readings is R(k+1)/R(k) × the
   * chains' gain ratio, independent of how accurately the current is known.
   */
  async seqRatio(vBox: number) {
    if (this.zero.some((z) => Number.isNaN(z))) {
      if (!(await this.ask('Ratio transfer needs the zeros. Open the current loop (load input off, CH1 off), then Continue: the zero sequence runs first.'))) throw new Aborted('aborted');
      await this.everythingOff();
      await this.seqZero();
    }
    const map = this.chainMap;
    let wiredBox = false;
    let product = 1;
    for (let k = 0; k + 1 < afe.TAPS; k++) {
      this.checkAbort();
      const a = 0.8 * afe.FS_A[k + 1];
      const useLoad = a >= 10e-3; // the KEL102 holds ≥ 10 mA steadily; below: the box
      if (useLoad) {
        await this.gotoRange(k);
        await this.psuOn(5.0, Math.min(3.2, a * 1.3 + 0.01));
        await this.mustLoad(a);
      } else {
        if (!wiredBox) {
          await this.everythingOff();
          await this.gotoRange(k);
          if (!(await this.ask(`Rewire for the decade box (§7): CH1 (+) → I+, I− → box → CH1 (−); load out of the loop (its input across CH1 to read the voltage, input OFF). CH1 will be set to ${vBox.toFixed(2)} V.`)))
            throw new Aborted('aborted');
          wiredBox = true;
        }
        await this.gotoRange(k);
        const rbox = vBox / a;
        // CH1 off while the box is dialled: a decade passes through lower values on the way
        await this.psu.output(1, false);
        this.psuOut[0] = false;
        if (!(await this.ask(`Tap ${k + 1} → ${k + 2}: CH1 is off. Dial the box to ${rbox.toFixed(0)} Ω (${afe.siAmps(a)} at ${vBox.toFixed(2)} V; ${(a * a * rbox * 1e3).toFixed(1)} mW in the box), then Continue: CH1 comes back on.`)))
          throw new Aborted('aborted');
        if (this.psu.simulated) {
          simBench.rBox = rbox;
          simBench.changed();
        }
        // the SPD3303C sets current in 10 mA steps: 10 mA is its smallest limit
        await this.psuOn(vBox, 0.01);
      }
      await sleep(500);
      await this.gotoRange(k);
      await sleep(100);
      const m1 = await this.measure(k, this.measMs);
      await this.gotoRange(k + 1);
      await sleep(100);
      const m2 = await this.measure(k + 1, this.measMs);
      await this.gotoRange(k);
      await sleep(100);
      const m3 = await this.measure(k, this.measMs);
      if (!m1.ok || !m2.ok || !m3.ok) throw new Aborted(!m1.ok ? m1.why : !m2.ok ? m2.why : m3.why);
      const cK = (m1.mean + m3.mean) / 2 - this.zeroOf(k);
      const cK1 = m2.mean - this.zeroOf(k + 1);
      let ratio = cK1 / cK;
      // a voltage source and a box: the burden is 10× larger on tap k+1, so its current is lower
      // by (V - b_k1)/(V - b_k); with the load in CC the current doesn't move
      let corr = 1;
      if (!useLoad) corr = (vBox - afe.burdenV(a, k)) / (vBox - afe.burdenV(a, k + 1));
      ratio *= corr;
      const nom = afe.ampsToCode(1, k + 1, map) / afe.ampsToCode(1, k, map);
      const e = (ratio / nom - 1) * 100;
      product *= ratio / nom;
      const note = `reading ratio ${ratio.toFixed(5)} (nominal ${nom.toFixed(4)}): ${e >= 0 ? '+' : ''}${e.toFixed(3)} %; drift on tap ${k + 1} ${(m3.mean - m1.mean).toFixed(2)} codes; burden corr ${corr.toFixed(5)}; tap ${k + 2} vs tap 1: ${((product - 1) * 100).toFixed(3)} %`;
      this.addRow({ test: `ratio ${k + 1}→${k + 2}`, tap: k + 2, iRef: NaN, code: cK1, sd: m2.sd, n: m2.n, dirty: 0, iNeb: afe.codeToAmps(cK1, k + 1, map), err: e, note });
      this.log(note);
      this.status = `ratio ${k + 1}→${k + 2}: ${e.toFixed(3)} %`;
    }
  }

  /** §8: comparator trip points on taps 1-3: the raw pins while the load ramps */
  async seqComparators(tap: number) {
    const r = tap - 1;
    const fs = afe.FS_A[r];
    await this.gotoRange(r);
    await this.psuOn(5.0, Math.min(3.2, fs * 1.15 + 0.01));
    const d = this.host.dev();
    if (!d) throw new Aborted('not connected');
    const pins = async () => (await dbg.range(d).catch(() => null))?.dec.pins ?? null;
    const ramp = async (from: number, to: number, step: number, what: string) => {
      let prev: number | null = null;
      for (let p = from; step > 0 ? p <= to + 1e-9 : p >= to - 1e-9; p += step) {
        this.checkAbort();
        await this.mustLoad(p * fs);
        await sleep(250);
        const now = await pins();
        if (now === null) continue;
        if (prev !== null && now !== prev) {
          const ref = await this.loadAmps(2);
          const note = `pins 0x${prev.toString(16).padStart(2, '0')} → 0x${now.toString(16).padStart(2, '0')} at ${((ref / fs) * 100).toFixed(2)} % FS (load ${afe.siAmps(ref)}); expect UP at ${afe.COMP_UP * 100} %, DOWN at ${(afe.COMP_DOWN * 100).toFixed(1)} %`;
          this.addRow({ test: `comp ${what} tap${tap}`, tap, iRef: ref, code: NaN, sd: NaN, n: 0, dirty: 0, iNeb: NaN, err: (ref / fs) * 100, note });
          this.log(note);
        }
        prev = now;
        this.status = `comparators tap ${tap}: ${(p * 100).toFixed(1)} % FS, pins 0x${now.toString(16).padStart(2, '0')}`;
      }
    };
    await ramp(0.8, 1.0, 0.0025, 'up');
    await ramp(1.0, 0.8, -0.0025, 'up-release');
    await ramp(0.1, 0.02, -0.001, 'down');
    await ramp(0.02, 0.1, 0.001, 'down-release');
  }

  // ---- playbook: building -----------------------------------------------------------

  private buildCheck(v: Step[], needLoad: boolean, needPsu: boolean) {
    const c = mk('check', 'Check: Nebulosa, stream, instruments', 'The Nebulosa connected, the stream on with the ADC as source, and the instruments open.');
    c.needLoad = needLoad;
    c.needPsu = needPsu;
    v.push(c);
    v.push(mk('alloff', 'Load input and CH1 off'));
    v.push(mk('timing', 'Switch overlap 0.5 µs', 'One make-before-break overlap for every switch (bring-up).'));
  }

  /** DMM tap identification: which FET each tap closes, no current source (§3.2) */
  private buildDmm(v: Step[]) {
    this.buildCheck(v, false, false);
    v.push(
      mk(
        'info',
        'Disconnect I+ / I−',
        'Nothing on I+ / I−: no supply, no load, no box, no short. DMM on ohms, RED on I−, BLACK on I+ (reverse polarity: the current goes through the FET that is on, the body diodes stay off).',
      ),
    );
    // R_cum + the switch: tap 1 18.7 mΩ, tap 2 ~0.12, tap 3 ~1.03, 2N7002E 1-3 Ω, + leads and tolerance
    const lo = [0, 0, 0.6, 10.3, 100, 985],
      hi = [0.5, 0.6, 2.0, 15, 106, 1020];
    for (let r = 0; r < afe.TAPS; r++) {
      const t = mk('tap', `Tap ${r + 1}`);
      t.tap = r;
      v.push(t);
      const d = mk(
        'dmm',
        `DMM: I− to I+ on tap ${r + 1}`,
        `Read the DMM (ohms, red on I−, black on I+) and enter it. Expect ${+afe.R_CUM[r].toPrecision(3)} Ω (R_cum) plus the switch${r < 3 ? ' and Qi: taps 1 and 2 read as a short' : ' (2N7002E: 1-3 Ω)'}.`,
      );
      d.tap = r;
      d.lo = lo[r];
      d.hi = hi[r];
      d.unit = 'Ω';
      v.push(d);
    }
    const back = mk('tap', 'Back to tap 1');
    back.tap = 0;
    v.push(back);
  }

  /** one tap: zero, then 10 / 50 / 85 % FS (85 %: below the UP comparator's 90 %) */
  private buildTap(v: Step[], r: number, vbox: number, wiring: { now: number }) {
    const pts = [0.1, 0.5, 0.85];
    const fs = afe.FS_A[r];
    const load = r <= 1; // the KEL102 is a reference on taps 1-2 only
    v.push(mk('alloff', `Tap ${r + 1} (${afe.FS_LABEL[r]} FS): load and CH1 off`));
    const want = load ? 1 : 2;
    if (wiring.now !== want) {
      v.push(mk('info', load ? 'Wire load mode' : 'Wire box mode', load ? WIRE_LOAD : WIRE_BOX));
      wiring.now = want;
    }
    const t = mk('tap', `Tap ${r + 1}`, 'Manual range, stepping down one tap at a time.');
    t.tap = r;
    v.push(t);
    const z = mk('zero', `Tap ${r + 1}: zero, loop open`, 'CH1 off, no current: the tap’s offset (subtracted from every later reading) and its noise.');
    z.tap = r;
    v.push(z);
    if (load) {
      const lim = Math.min(3.2, pts[2] * fs * 1.15 + 0.01);
      const p = mk('psu', `CH1: 5.00 V, limit ${afe.siAmps(lim)}, ON`);
      p.v = 5.0;
      p.a = lim;
      p.on = true;
      v.push(p);
      for (const q of pts) {
        const l = mk('load', `Load: CC ${afe.siAmps(q * fs)}, input ON (${(q * 100).toFixed(0)} % FS)`);
        l.a = q * fs;
        l.on = true;
        l.tap = r;
        v.push(l);
        const m = mk('meas', `Measure tap ${r + 1} at ${(q * 100).toFixed(0)} % FS`, 'Reference: the KEL102’s current. Read: the ADC, offset-corrected, nominal gain.');
        m.tap = r;
        m.iSet = q * fs;
        m.ref = 'load';
        v.push(m);
      }
    } else {
      for (const q of pts) {
        const i = q * fs,
          rb = afe.boxRound(vbox / i);
        const off = mk('psu', 'CH1 OFF (before dialling the box)');
        off.on = false;
        v.push(off);
        const b = mk(
          'box',
          `Decade box: ${afe.ohms(rb)}`,
          `CH1 is off. Dial the box to ${afe.ohms(rb)} (${afe.siAmps(vbox / rb)} at ${vbox.toFixed(2)} V, ${((vbox * vbox) / rb * 1e3).toFixed(1)} mW in the box: check its rating), then enter what you dialled.`,
        );
        b.boxOhm = rb;
        b.tap = r;
        v.push(b);
        // CH1's limit above the planned current (10 mA steps on the SPD3303C), the box sets the current
        const lim = Math.max(0.01, Math.ceil(((vbox / rb) * 1.3 + 0.002) / 0.01) * 0.01);
        const on = mk('psu', `CH1: ${vbox.toFixed(2)} V, limit ${(lim * 1e3).toFixed(0)} mA, ON`);
        on.v = vbox;
        on.a = lim;
        on.on = true;
        v.push(on);
        const m = mk('meas', `Measure tap ${r + 1} at ${(q * 100).toFixed(0)} % FS`, 'Reference: V (the KEL102 as voltmeter, else CH1’s readback) / the box. Read: the ADC, offset-corrected, nominal gain.');
        m.tap = r;
        m.iSet = vbox / rb;
        m.ref = 'box';
        v.push(m);
      }
    }
    v.push(mk('alloff', `Tap ${r + 1} done: load and CH1 off`));
  }

  pbBuild() {
    const v: Step[] = [];
    const k = this.pbKind;
    if (k === 0) this.buildDmm(v);
    else {
      const from = k === 1 ? 0 : k - 2,
        to = k === 1 ? afe.TAPS - 1 : k - 2;
      this.buildCheck(v, from <= 1, true);
      const wiring = { now: 0 };
      for (let r = from; r <= to; r++) this.buildTap(v, r, Math.min(30, Math.max(2, this.pbBoxV)), wiring);
      const t = mk('tap', 'Back to tap 1', 'Done: the safe tap.');
      t.tap = 0;
      v.push(t);
    }
    this.steps = v;
    this.cur = 0;
    this.pbName = PLAYBOOKS[k];
    this.boxActual = NaN;
  }

  // ---- playbook: running ----------------------------------------------------------------

  private finish(i: number, state: StepState, result: string, expect = NaN, read = NaN, errFs = NaN, tol = NaN) {
    const s = this.steps[i];
    if (!s) return;
    Object.assign(s, { state, result, expect, read, errFs, tol });
    if ((state === 'pass' || state === 'done' || state === 'skipped') && this.cur === i) this.cur = i + 1;
  }

  /** the step at i; automatic steps after it too, while auto-run is on */
  async pbExec(i: number) {
    if (this.executing || this.busy) return;
    this.executing = true;
    this.busy = true;
    this.abort = false;
    try {
      for (;;) {
        await this.pbExecNow(i);
        const next = this.steps[i + 1];
        if (!(this.pbAuto && !this.abort && this.cur === i + 1 && next && this.steps[i].state !== 'fail' && !operatorStep(next.kind))) break;
        i++;
      }
    } finally {
      this.busy = false;
      this.executing = false;
    }
  }

  private async pbExecNow(i: number) {
    const s = this.steps[i];
    if (!s) return;
    s.state = 'running';
    try {
      await this.runStep(i, s);
    } catch (e) {
      this.finish(i, 'fail', e instanceof Error ? e.message : String(e));
    }
  }

  private async runStep(i: number, s: Step) {
    const d = this.host.dev();
    switch (s.kind) {
      case 'check': {
        if (!d) return this.finish(i, 'fail', 'Nebulosa not connected');
        if (s.needPsu && !this.psu.open) return this.finish(i, 'fail', 'SPD3303C not open (Bench)');
        if (s.needLoad && !this.load.open) return this.finish(i, 'fail', 'KEL102 not open (Bench)');
        if (!this.host.streaming()) {
          await this.host.startAdc();
          for (let t = 0; t < 30 && !this.host.streaming(); t++) await sleep(100);
        }
        if (!this.host.streaming()) return this.finish(i, 'fail', 'the stream didn’t start');
        await sleep(200);
        await this.readRange();
        if (this.manualRange < 0) await this.gotoRange(0);
        const m = await this.measure(this.manualRange, 50);
        return this.finish(i, 'pass', `streaming${m.ok ? '' : ' (no ADC samples yet)'}, manual tap ${this.manualRange + 1}`);
      }
      case 'timing':
        return this.finish(i, 'pass', await this.setOverlap());
      case 'alloff':
        return this.finish(i, (await this.everythingOff()) ? 'pass' : 'fail', 'load input off, CH1 off');
      case 'tap': {
        await this.gotoRange(s.tap);
        const r = d ? await dbg.range(d).catch(() => null) : null;
        if (r && r.dec.taps !== 1 << s.tap) return this.finish(i, 'fail', `EN_TAP 0x${r.dec.taps.toString(16).padStart(2, '0')}, expected only tap ${s.tap + 1}`);
        return this.finish(i, 'pass', `manual tap ${s.tap + 1}, EN_TAP ${s.tap + 1} only`);
      }
      case 'psu': {
        if (s.on) {
          if (this.manualRange < 0) return this.finish(i, 'fail', 'no manual tap: CH1 stays off');
          await this.psu.set(1, s.v, s.a);
        }
        await this.psu.output(1, s.on);
        this.psuOut[0] = s.on;
        await sleep(s.on ? 400 : 100);
        const m = await this.psu.measure(1).catch(() => ({ v: NaN, a: NaN }));
        this.psuV[0] = m.v;
        this.psuI[0] = m.a;
        if (s.on && !Number.isNaN(m.v) && Math.abs(m.v - s.v) > 0.05 + 0.01 * s.v)
          return this.finish(i, 'fail', `CH1 reads ${m.v.toFixed(3)} V, set ${s.v.toFixed(2)} V: in current limit? (${m.a.toFixed(3)} A)`, s.v, m.v);
        return this.finish(i, 'pass', s.on ? `CH1 ${m.v.toFixed(3)} V, ${m.a.toFixed(3)} A` : 'CH1 off', s.on ? s.v : NaN, m.v);
      }
      case 'load': {
        if (!(await this.loadCc(s.a, s.on))) return this.finish(i, 'fail', 'refused by the interlock, or the load didn’t answer');
        await sleep(400);
        const a = await this.loadAmps(2);
        const tol = 0.0005 * s.a + 0.0015 + 0.0002;
        if (s.on && !(Math.abs(a - s.a) <= tol)) return this.finish(i, 'fail', `load draws ${afe.siAmps(a)}, set ${afe.siAmps(s.a)}: CH1 limit or compliance?`, s.a, a);
        return this.finish(i, 'pass', `load ${afe.siAmps(a)}`, s.a, a);
      }
      case 'zero': {
        if (this.manualRange !== s.tap) return this.finish(i, 'fail', `the tap is ${this.manualRange + 1}, not ${s.tap + 1}`);
        await sleep(150);
        const m = await this.measure(s.tap, Math.max(200, this.measMs));
        if (!m.ok) return this.finish(i, 'fail', m.why);
        this.zero[s.tap] = m.mean;
        this.zeroSd[s.tap] = m.sd;
        const fsCode = afe.ampsToCode(afe.FS_A[s.tap], s.tap, this.chainMap);
        const off = (m.mean / fsCode) * 100;
        const w = this.makeRow(`pb zero tap${s.tap + 1}`, s.tap, m, NaN);
        w.code = m.mean;
        w.iNeb = afe.codeToAmps(m.mean, s.tap, this.chainMap);
        w.err = off;
        w.note = `offset ${off.toFixed(3)} % FS, FS/rms ${m.sd > 0 ? (fsCode / m.sd).toFixed(0) : '—'} (full band)`;
        this.addRow(w);
        return this.finish(
          i,
          Math.abs(off) <= 0.5 ? 'pass' : 'fail',
          `offset ${m.mean.toFixed(1)} codes = ${off >= 0 ? '+' : ''}${off.toFixed(3)} % FS${Math.abs(off) > 0.1 ? ' (above 0.1 %: look at it)' : ''}; noise ${m.sd.toFixed(2)} codes rms = FS/${m.sd > 0 ? (fsCode / m.sd).toFixed(0) : '—'}`,
          0,
          w.iNeb,
          off,
          0.5,
        );
      }
      case 'meas': {
        const r = s.tap;
        if (this.manualRange !== r) return this.finish(i, 'fail', `the tap is ${this.manualRange + 1}, not ${r + 1}`);
        const fs = afe.FS_A[r];
        let ref = NaN,
          refUnc = 0,
          how = '';
        if (s.ref === 'load') {
          ref = await this.loadAmps(3);
          refUnc = 0.0005 * Math.abs(ref) + 0.0015;
          how = 'KEL102 current';
        } else {
          const rb = this.boxActual;
          if (!(rb > 0)) return this.finish(i, 'fail', 'no box value entered');
          let vv = NaN;
          const lm = this.load.open ? await this.load.measure().catch(() => null) : null;
          if (lm && lm.v > 0.1) {
            vv = lm.v;
            how = 'KEL102 voltage / box';
            refUnc = (0.00025 * vv + 0.0045) / vv;
          } else {
            const pm = await this.psu.measure(1).catch(() => ({ v: NaN, a: NaN }));
            vv = pm.v;
            how = 'CH1 readback / box (no KEL102 voltage: ±0.5 %)';
            refUnc = (0.005 * vv + 0.02) / vv;
          }
          if (!(vv > 0)) return this.finish(i, 'fail', 'no loop voltage: CH1 on?');
          const i0 = vv / rb;
          ref = (vv - afe.burdenV(i0, r)) / rb;
          refUnc = ref * (refUnc + this.pbBoxTol / 100);
        }
        await sleep(200);
        const m = await this.measure(r, this.measMs);
        if (!m.ok) return this.finish(i, 'fail', m.why);
        const w = this.makeRow(`pb tap${r + 1}`, r, m, ref, `${((s.iSet / fs) * 100).toFixed(0)} % FS, ref ${how}`);
        this.addRow(w);
        const tol = 0.001 * fs + (this.pbGainTol / 100) * Math.abs(ref) + refUnc;
        const err = w.iNeb - ref;
        const dirty = m.n + m.dirty ? m.dirty / (m.n + m.dirty) : 0;
        const pass = Math.abs(err) <= tol && dirty < 0.5;
        return this.finish(
          i,
          pass ? 'pass' : 'fail',
          `expect ${afe.siAmps(ref)} (${how}), read ${afe.siAmps(w.iNeb)}: ${((err / fs) * 100).toFixed(3)} % FS, ${ref ? ((err / ref) * 100).toFixed(3) : '0'} % of reading; limit ±${afe.siAmps(tol)}; noise ${m.sd.toFixed(2)} codes rms${dirty >= 0.5 ? '; most samples DIRTY (comparator asserted?)' : ''}`,
          ref,
          w.iNeb,
          (err / fs) * 100,
          (tol / fs) * 100,
        );
      }
      default:
        return; // operator steps finish from the dialog
    }
  }

  /** the operator's Done on step i (entry: the box / DMM value) */
  async pbOperatorDone(i: number, entry: number) {
    const s = this.steps[i];
    if (!s) return;
    if (s.kind === 'box') {
      if (!(entry > 0)) return;
      this.boxActual = entry;
      if (this.psu.simulated) {
        simBench.rBox = entry;
        simBench.changed();
      }
      const dev = (entry / s.boxOhm - 1) * 100;
      this.finish(i, Math.abs(dev) < 5 ? 'done' : 'fail', `box ${afe.ohms(entry)}${Math.abs(dev) < 5 ? '' : ` (${dev.toFixed(0)} % off the plan: redo)`}`, s.boxOhm, entry);
    } else if (s.kind === 'dmm') {
      const pass = entry >= s.lo && entry <= s.hi;
      this.finish(i, pass ? 'pass' : 'fail', `${+entry.toPrecision(4)} ${s.unit}, expected ${s.lo} .. ${s.hi} ${s.unit}`, (s.lo + s.hi) / 2, entry);
      this.addRow({ test: `pb dmm tap${s.tap + 1}`, tap: s.tap + 1, iRef: NaN, code: NaN, sd: NaN, n: 0, dirty: 0, iNeb: NaN, err: NaN, note: `I− to I+ ${+entry.toPrecision(4)} Ω (expect ${s.lo} .. ${s.hi})${pass ? '' : ' FAIL'}` });
    } else this.finish(i, 'done', 'done');
    const next = this.steps[i + 1];
    if (this.pbAuto && this.cur === i + 1 && next && !operatorStep(next.kind)) await this.pbExec(i + 1);
  }

  skip(i: number) {
    this.finish(i, 'skipped', 'skipped');
  }

  // ---- results --------------------------------------------------------------------------

  csv(): string {
    const q = (s: string) => `"${s.replace(/"/g, "'")}"`;
    const num = (x: number, d: number) => (Number.isNaN(x) ? '' : x.toFixed(d));
    const g = (x: number) => (Number.isNaN(x) ? '' : x.toPrecision(9));
    const lines = ['test,tap,i_ref_A,code,rms_codes,n,dirty_frac,i_nominal_A,err_pct,note'];
    for (const w of this.rows) lines.push([w.test, w.tap, g(w.iRef), num(w.code, 4), num(w.sd, 4), w.n, num(w.dirty, 4), g(w.iNeb), num(w.err, 5), q(w.note)].join(','));
    return lines.join('\n') + '\n';
  }

  forgetZeros() {
    this.zero = [NaN, NaN, NaN, NaN, NaN, NaN];
    this.zeroSd = [NaN, NaN, NaN, NaN, NaN, NaN];
  }

  // ---- the demo device ---------------------------------------------------------------

  /** with simulated instruments on the demo: the loop current as the demo's ADC codes (the gateware's ×25 map) */
  syncDemo() {
    if (!this.host.demoBench) return;
    const sim = this.psu.simulated || this.load.simulated;
    const r = this.manualRange;
    if (!sim || r < 0) return this.host.demoBench(null);
    const i = simBench.solve().i;
    const code = afe.ampsToCode(i, r, 'x25') + 6; // a few codes of offset
    const fs = afe.FS_A[r];
    const c = afe.chainOf(r, 'x25');
    const pins = (i > afe.COMP_UP * fs ? 1 << (2 * c) : 0) | (i < afe.COMP_DOWN * fs ? 1 << (2 * c + 1) : 0);
    this.host.demoBench({ range: r, code, pins });
  }
}
