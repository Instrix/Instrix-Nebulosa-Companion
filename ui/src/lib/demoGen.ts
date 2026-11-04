// The demo device's signal: real NBP DATA messages (format-5 FPGA words) at
// 2 MS/s from a synthetic load. Runs where the pipeline does (the worker), so
// making the samples doesn't cost the UI thread anything.

export const RATE = 2_000_000;
export const TICK_MS = 40;
const WORDS_PER_DATA = 507;
const RANGE_FS = [2, 0.2, 20e-3, 2e-3, 200e-6, 20e-6];

/**
 * What the demo device is connected to (the showcases pick one):
 *   iot      a sensor: idle ~200 µA, a 40 ms burst of ~25 mA every 500 ms, now and then a gap
 *   ble      a BLE beacon on a coin cell: asleep at 3 µA, an advertising event every 100 ms
 *   pwm      a motor driver from 5 V: 2 kHz PWM at 30 % duty
 *   glitch   a steady 12 mA load with a ~100 mA, 80 µs spike every second or two
 *   sleep    a microcontroller at ~1 µA, waking for 8 ms at 4 mA each second
 *   boot     switched on every 3 s: inrush, boot, flash writes, idle
 *   battery  a Li-ion cell (a whole discharge in 4 minutes) under 120 mA radio bursts
 *   target   a microcontroller under attack: idle at 2 mA, running what the
 *            side-channel tab sends it (DemoTarget: a leaky AES, a password check)
 */
export type DemoScenario = 'iot' | 'ble' | 'pwm' | 'glitch' | 'sleep' | 'boot' | 'battery' | 'target';

/**
 * Something the demo target does, as data (it crosses into the worker): the
 * current over it is built from this where the generator runs.
 *   wake   waking from 2 mA to ~6 mA over 20 µs
 *   aes    wake, then from `sb`: 16 S-box lookups of 16 µs, each 0.5 mA more
 *          per bit of `leak[k]` (its Hamming weight), then the other rounds
 *   pulse  wake for `after`, then a flat `level`
 */
export type OpSpec =
  | { kind: 'wake' }
  | { kind: 'aes'; sb: number; leak: number[] }
  | { kind: 'pulse'; after: number; level: number };

const wake = (s: number) => 2e-3 + 4e-3 * Math.min(1, s / 20e-6);

/** the op's current (A) at s into it */
export function opCurrent(op: OpSpec): (s: number) => number {
  switch (op.kind) {
    case 'wake':
      return wake;
    case 'pulse':
      return (s) => (s < op.after ? wake(s) : op.level);
    case 'aes':
      return (s) => {
        if (s < 100e-6) return wake(s);
        const k = Math.floor((s - op.sb) / 16e-6);
        if (s >= op.sb && k < 16) {
          const u = s - op.sb - k * 16e-6;
          return 6e-3 + (u >= 4e-6 && u < 12e-6 ? 0.5e-3 * op.leak[k] : 0.4e-3);
        }
        return 6e-3 + 0.6e-3 * Math.sin(s * 9e4) * Math.sin(s * 2.3e4);
      };
  }
}

/** an op queued on the target, in generator time */
interface DemoOp {
  start: number;
  dur: number;
  /** A, at s into the op */
  current: (s: number) => number;
  done: () => void;
}

/** roughly normal, σ = 1 */
const gauss = () => (Math.random() + Math.random() + Math.random() - 1.5) * 2;

const noise = (x: number, frac: number) => x * (1 + (Math.random() - 0.5) * frac);

/** the smallest range whose full scale holds `a` with some margin */
function rangeFor(a: number) {
  for (let r = RANGE_FS.length - 1; r >= 0; r--) if (a < RANGE_FS[r] * 0.9) return r;
  return 0;
}

export class Generator {
  scenario: DemoScenario = 'iot';
  /** the target's queue (scenario 'target'), in start order */
  private ops: DemoOp[] = [];

  /** generator time now, s */
  now() {
    return this.ts / RATE;
  }

  /** runs `op` on the target soon after the last one (a command's turnaround: 1 to 1.5 ms) */
  schedule(dur: number, op: OpSpec): Promise<void> {
    const current = opCurrent(op);
    const last = this.ops[this.ops.length - 1];
    const start = Math.max(this.now(), last ? last.start + last.dur : 0) + 1e-3 + Math.random() * 0.5e-3;
    return new Promise((done) => this.ops.push({ start, dur, current, done }));
  }
  /** glitch: the next spike's start, s */
  private glitchAt = 1;
  private ts = 0; // 500 ns periods (= samples at 2 MS/s)
  private blockSeq = 0; // FPGA block sequence
  private dataSeq = 0; // NBP DATA sequence
  private words: number[] = [];
  private inBlock = 0;
  private range = -1;
  private first = true;

  /** `n` samples' worth of DATA messages */
  next(n: number): Uint8Array[] {
    for (let k = 0; k < n; k++) {
      const t = this.ts / RATE;
      // now and then a gap: 15 ms of lost samples
      if (this.scenario === 'iot' && this.ts % (RATE * 2.7) < 1 && this.ts > 0) {
        this.ts += RATE * 0.015;
        this.range = -1; // a new block after the gap
      }
      const [i0, v0] = this.sample(t);
      const i = Math.max(0, i0);
      const v = Math.max(0, v0 + (Math.random() - 0.5) * 2e-3);
      const r = rangeFor(i);
      if (r !== this.range || this.inBlock >= 256) this.header(r);
      const ic = Math.min(16383, Math.round((i / RANGE_FS[r]) * 16384));
      const vc = Math.min(16383, Math.round(v / 1e-3));
      this.words.push(((1 << 31) | (ic << 14) | vc) >>> 0);
      this.inBlock++;
      this.ts++;
    }
    const out: Uint8Array[] = [];
    while (this.words.length >= WORDS_PER_DATA) out.push(this.data(this.words.splice(0, WORDS_PER_DATA)));
    return out;
  }

  /** the load's current (A) and voltage (V) at t, noise included */
  private sample(t: number): [number, number] {
    switch (this.scenario) {
      case 'ble': {
        // every 100 ms: the CPU and crystal wake (1 ms), then three TX bursts, one per advertising channel
        const ph = t % 0.1;
        let i = 3e-6;
        if (ph < 1e-3) i = 2.5e-3;
        else if (ph < 5.6e-3) {
          const k = Math.floor((ph - 1.2e-3) / 1.6e-3);
          const inTx = ph >= 1.2e-3 && k < 3 && ph - 1.2e-3 - k * 1.6e-3 < 0.45e-3;
          i = inTx ? 8.5e-3 + 0.4e-3 * Math.sin(ph * 4e4) : 1.2e-3;
        }
        i = noise(i, i < 1e-4 ? 0.2 : 0.05);
        return [i, 3.0 - i * 0.5];
      }
      case 'pwm': {
        // on: the winding's current ramps up; off: it freewheels, the supply gives only the logic's 2 mA
        const ph = (t * 2000) % 1;
        const i = noise(ph < 0.3 ? 0.08 + 0.06 * (ph / 0.3) : 2e-3, 0.03);
        return [i, 5 - i * 0.4];
      }
      case 'glitch': {
        if (t > this.glitchAt + 1e-3) this.glitchAt = t + 0.6 + Math.random() * 1.8;
        const g = t - this.glitchAt;
        const spike = g >= 0 && g < 80e-6 ? 0.085 * Math.sin((Math.PI * g) / 80e-6) : 0;
        const i = noise(12e-3 + 0.4e-3 * Math.sin(t * 2 * Math.PI * 50), 0.04) + spike;
        return [i, 3.3 - i * 2.5];
      }
      case 'sleep': {
        // each second: 8 ms awake; an RTC tick (50 µs) every 250 ms; asleep at ~0.9 µA
        const ph = t % 1;
        let i = 0.9e-6;
        if (ph < 8e-3) i = 4e-3 * (0.9 + 0.1 * Math.sin(ph * 3000));
        else if (ph % 0.25 < 50e-6) i = 600e-6;
        i = noise(i, i < 1e-5 ? 0.1 : 0.04);
        return [i, 1.8 - i * 0.2];
      }
      case 'boot': {
        // every 3 s: off for 0.8 s, then on: inrush into the capacitors, boot, flash writes, idle
        const ph = t % 3;
        if (ph < 0.8) return [0, 0];
        const s = ph - 0.8;
        let i = s < 0.35 ? 45e-3 : s < 0.5 ? 70e-3 : s % 0.1 < 5e-3 ? 20e-3 : 6e-3;
        i = noise(i, 0.06) + 0.6 * Math.exp(-s / 0.4e-3);
        return [i, 3.3 * (1 - Math.exp(-s / 0.2e-3)) - i * 0.3];
      }
      case 'battery': {
        // a Li-ion cell, 4.2 V full to ~3.3 V empty every 240 s; 0.35 Ω inside; 120 mA for 0.4 s every 2 s
        const soc = 1 - (t % 240) / 240;
        const ocv = 3.3 + 0.9 * soc ** 0.7 - 0.05 * Math.sin(soc * 9);
        const i = noise(t % 2 < 0.4 ? 0.12 : 8e-3, 0.05);
        return [i, ocv - i * 0.35];
      }
      case 'target': {
        while (this.ops.length && t >= this.ops[0].start + this.ops[0].dur) this.ops.shift()!.done();
        const op = this.ops[0];
        const i = (op && t >= op.start ? op.current(t - op.start) : 2e-3) + gauss() * 0.15e-3;
        return [i, 3.3 - i * 1.5];
      }
      default: {
        const phase = t % 0.5;
        const burst = phase < 0.04;
        const base = burst ? 25e-3 + 3e-3 * Math.sin(phase * 900) : 200e-6 + 20e-6 * Math.sin(t * 7);
        const i = noise(base, burst ? 0.08 : 0.15);
        return [i, 3.3 - i * 0.8];
      }
    }
  }

  private header(range: number) {
    this.range = range;
    this.inBlock = 0;
    const seq = this.blockSeq++ & 0xffff;
    this.words.push(((2 << 29) | (seq << 11) | (range << 2) | 2) >>> 0);
    this.words.push(((2 << 29) | (1 << 27) | (Math.floor(this.ts / 2 ** 24) & 0xffffff)) >>> 0);
    this.words.push(((2 << 29) | (2 << 27) | (this.ts & 0xffffff)) >>> 0);
  }

  private data(words: number[]): Uint8Array {
    const b = new Uint8Array(2048);
    const d = new DataView(b.buffer);
    const seq = this.dataSeq++;
    b[0] = 0x4e; // 'N'
    b[1] = 0x42; // 'B'
    b[2] = 0x80; // DATA
    d.setUint16(4, seq & 0xffff, true);
    d.setUint16(6, 2040, true);
    d.setUint32(8, seq, true);
    d.setUint32(12, 0, true);
    d.setUint16(16, words.length, true);
    d.setUint16(18, this.first ? 1 : 0, true); // FIRST
    this.first = false;
    words.forEach((w, k) => d.setUint32(20 + k * 4, w, false)); // FPGA words are big-endian
    return b;
  }
}

