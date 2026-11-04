// Synthetic device for UI work without hardware: open the UI with `?demo`.
//
// It builds real NBP DATA messages (format-5 FPGA words: block headers,
// timestamps, samples) at 2 MS/s and feeds them to the same WASM pipeline a
// WebUSB device would, so everything after the transport is the real thing.
// The load: a 3.3 V rail, idle at ~200 µA, waking every 500 ms for a 40 ms
// burst of ~25 mA with a ripple, plus noise; now and then a 15 ms gap.
//
// `?demo&sel=a,b` also puts a dual marker at [a, b] s before the newest
// sample (for screenshots); `&log=i,p`, `&trace=mean,range,sigma` set the view.

import type { Link, LinkEvents } from './device';
import { core, type ExportCfg, type AnalysisRequest, type AnalysisResult, type Calibration, type Meta, type RawSpanStats, type Reply, type TrigEvent, type TriggerCfg } from './nbp';
import { wasmEngine } from './sca';
import { aesEncrypt, hw, SBOX } from './aes';

const RATE = 2_000_000;
const TICK_MS = 40;
const WORDS_PER_DATA = 507;
const RANGE_FS = [2, 0.2, 20e-3, 2e-3, 200e-6, 20e-6];

export const demoRequested = typeof location !== 'undefined' && new URLSearchParams(location.search).has('demo');

/** `sel=a,b`: seconds before the newest sample */
export function demoSelection(): [number, number] | null {
  const s = new URLSearchParams(location.search).get('sel');
  if (!s) return null;
  const [a, b] = s.split(',').map(Number);
  return isFinite(a) && isFinite(b) ? [a, b] : null;
}

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

/** something the target does, in generator time: its current over it, and who's waiting */
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

class Generator {
  scenario: DemoScenario = 'iot';
  /** the target's queue (scenario 'target'), in start order */
  private ops: DemoOp[] = [];

  /** generator time now, s */
  now() {
    return this.ts / RATE;
  }

  /** runs `op` on the target soon after the last one (a command's turnaround: 1 to 1.5 ms) */
  schedule(dur: number, current: (s: number) => number): Promise<void> {
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

/**
 * A simulated board's software-update side (for trying the updater with no
 * hardware): a 1 MB flash, two 448 KB slots, UPDATE ops as the firmware's
 * update_app.c answers them.
 */
class FakeBoard {
  flash = new Uint8Array(1 << 20).fill(0xff);
  slotSize = 448 * 1024;
  active = 0;
  fwVersion = 0x000200;
  slots = [
    { kind: 1, len: 300_000, crc: 0x1234abcd, id: 0x4e42000b },
    { kind: 0, len: 0, crc: 0, id: 0 },
  ];
  msg = '';

  // ---- board commands: no barrel jack, a USB-C PD charger at 5 V that offers 12 V
  streaming = true; // the demo starts streaming
  apwr = [false, false]; // H, L
  pdMv = 5000;
  pwrSel = 0;
  lcd = 1;
  rtcOffset = -437; // the board's clock is behind
  tests = 0;
  readonly t0 = Date.now();

  boardStatus(): Uint8Array<ArrayBuffer> {
    const b = new Uint8Array(200);
    const d = new DataView(b.buffer);
    const jitter = () => Math.round((Math.random() - 0.5) * 30);
    const apwrOk = this.pdMv >= 12000;
    d.setUint32(0, 0x4e440001, true);
    d.setUint32(4, Date.now() - this.t0 + 3_600_000, true);
    d.setUint32(8, 400e6, true);
    d.setUint32(12, 100e6, true);
    const flags = 0x7 | 1 << 10 | 1 << 11 | 1 << 14 | 1 << 15 | 1 << 19 | 1 << 21 | (this.apwr[0] ? 1 << 6 : 0) | (this.apwr[1] ? 1 << 7 : 0) | (apwrOk ? 1 << 20 : 0);
    d.setUint32(16, flags, true);
    d.setUint32(20, Math.round(Date.now() / 1000) + this.rtcOffset, true);
    d.setUint16(24, 0, true);
    d.setUint16(26, this.pdMv + jitter(), true);
    d.setUint16(28, this.pdMv - 80 + jitter(), true);
    b[34] = this.pwrSel;
    b[36] = 2;
    b[37] = this.lcd;
    d.setUint32(40, 92, true);
    d.setUint32(44, 300_000, true);
    d.setUint32(48, 0x4e42000b, true);
    d.setUint32(52, 0xef4014, true);
    d.setUint32(56, 1 << 20, true);
    d.setUint32(80, this.tests, true);
    [100e6, 412, 32769, this.pdMv - 80, 0xef4014, 0, 2, 0, 0, 0x4e42000b, 0x7c131, this.pdMv].forEach((v, k) => d.setUint32(84 + 4 * k, v, true));
    b[132] = 4;
    b[133] = 1;
    b[134] = 2;
    const pdos = [[5000, 3000], [9000, 3000], [12000, 3000], [15000, 3000], [20000, 3250]];
    b[135] = pdos.length;
    d.setUint16(136, this.pdMv, true);
    d.setUint16(138, 3000, true);
    d.setUint16(140, this.pdMv, true);
    b[142] = pdos.findIndex(([mv]) => mv === this.pdMv) + 1;
    pdos.forEach(([mv, ma], k) => d.setUint32(144 + 4 * k, ((mv / 50) << 10) | (ma / 10), true));
    d.setUint32(172, 2, true);
    return b;
  }

  boardSet(item: number, v: number): number {
    switch (item) {
      case 1:
      case 2:
        if (v && this.pdMv < 12000) return 7; // NO_POWER
        this.apwr[item === 1 ? 0 : 1] = !!v;
        return 0;
      case 5:
        if (v > 2) return 3;
        this.pwrSel = v;
        return 0;
      case 7:
        this.lcd = v;
        return 0;
      case 8:
        this.rtcOffset = v - Math.round(Date.now() / 1000);
        return 0;
      case 9:
        if (this.streaming) return 5;
        if (![5000, 9000, 12000, 15000, 20000].includes(v)) return 3;
        this.pdMv = v;
        if (v < 12000) this.apwr = [false, false];
        return 0;
    }
    return 0;
  }

  info(result = 0): Uint8Array<ArrayBuffer> {
    const b = new Uint8Array(136);
    const d = new DataView(b.buffer);
    d.setUint32(0, this.fwVersion, true);
    d.setUint32(4, 0x5a3f9e01, true);
    d.setUint32(8, 211_000, true);
    new TextEncoder().encodeInto('Nov  3 2026 18:00:00', b.subarray(16, 40));
    d.setUint32(40, 0x000100, true);
    b[44] = this.active;
    b[45] = 1 - this.active;
    b[47] = result;
    d.setUint32(48, this.slotSize, true);
    this.slots.forEach((sl, k) => {
      const o = 52 + k * 16;
      b[o] = sl.kind;
      d.setUint32(o + 4, sl.len, true);
      d.setUint32(o + 8, sl.crc, true);
      d.setUint32(o + 12, sl.id, true);
    });
    new TextEncoder().encodeInto(this.msg, b.subarray(88, 135));
    return b;
  }

  request(cmd: number, p: Uint8Array): Reply {
    const d = new DataView(p.buffer, p.byteOffset, p.byteLength);
    const ok = (payload: Uint8Array = new Uint8Array(), status = 0): Reply => ({ ty: cmd | 0x40, status, payload });
    const addr = p.length >= 8 ? d.getUint32(0, true) : 0;
    const len = p.length >= 8 ? d.getUint32(4, true) : 0;
    const activeRange = (a: number, n: number) => a < (this.active + 1) * this.slotSize && a + n > this.active * this.slotSize;
    switch (cmd) {
      case 0x10:
        this.streaming = true;
        return ok();
      case 0x11:
        this.streaming = false;
        return ok();
      case 0x38:
        return ok(this.boardStatus());
      case 0x39: {
        const st = this.boardSet(p[0], d.getUint32(4, true));
        return ok(this.boardStatus(), st);
      }
      case 0x3a: {
        if (this.streaming) return ok(this.boardStatus(), 5);
        // every test passes (SD: no card, skipped)
        this.tests = 0x555555 & ~(3 << 12) | (3 << 12);
        return ok(this.boardStatus());
      }
      case 0x3d: // FLASH_ERASE
        if ((addr | len) & 4095) return ok(undefined, 3);
        if (activeRange(addr, len)) return ok(undefined, 8);
        this.flash.fill(0xff, addr, addr + len);
        return ok();
      case 0x3c: // FLASH_WRITE
        if (len > 256 || (addr & 255) + len > 256) return ok(undefined, 3);
        if (activeRange(addr, len)) return ok(undefined, 8);
        for (let k = 0; k < len; k++) this.flash[addr + k] &= p[8 + k];
        return ok();
      case 0x3b: {
        // FLASH_READ
        const out = new Uint8Array(8 + len);
        out.set(p.subarray(0, 8));
        out.set(this.flash.subarray(addr, addr + len), 8);
        return ok(out);
      }
      case 0x3f: {
        // UPDATE
        const o = p[0], slot = p[1], n = d.getUint32(4, true), crc = d.getUint32(8, true);
        if (o === 0) return ok(this.info());
        const data = this.flash.subarray(slot * this.slotSize, slot * this.slotSize + n);
        if (o === 1 || o === 2) {
          if (core.crc32(data) !== crc) {
            this.msg = 'CRC mismatch: the slot holds something else';
            return ok(this.info(6), 6);
          }
          if (o === 1) {
            this.slots[slot] = { kind: 1, len: n, crc, id: 0x4e42000c };
            this.active = slot;
            this.msg = 'gateware tested (DONE, ID 4E42000C): active';
          } else {
            const c = core.firmware_check(data);
            if (!c.ok) {
              this.msg = c.error;
              return ok(this.info(3), 3);
            }
            this.slots[slot] = { kind: 2, len: n, crc, id: c.version };
            this.fwVersion = c.version;
            this.msg = `firmware ${c.version_str} staged: restarting to install`;
          }
          return ok(this.info());
        }
        if (o === 3) {
          this.active = 1 - this.active;
          this.msg = 'previous gateware active again';
          return ok(this.info());
        }
        return ok(this.info());
      }
    }
    return ok();
  }
}

/** A Link to the synthetic device; most commands are accepted and ignored. */
export class DemoLink implements Link {
  readonly description = 'Demo device (synthetic, 2 MS/s)';
  private pipeline = new core.Pipeline();
  readonly sca = wasmEngine(() => this.pipeline);
  private gen = new Generator();
  private timers: ReturnType<typeof setInterval>[] = [];
  /** STREAM_STOP pauses the data, STREAM_START resumes it (it starts streaming, like a connected board) */
  private streaming = true;

  constructor(ev: LinkEvents) {
    let sent = -1;
    this.timers.push(
      setInterval(() => {
        if (!this.streaming) return;
        for (const m of this.gen.next((RATE * TICK_MS) / 1000)) this.pipeline.push(0, m);
        const meta = this.pipeline.meta() as Meta;
        if (meta.version !== sent) {
          sent = meta.version;
          ev.onMeta(meta);
        }
      }, TICK_MS),
    );
  }

  private board = new FakeBoard();

  async request(cmd: number, payload = new Uint8Array()): Promise<Reply> {
    if (cmd === 0x10) this.streaming = true;
    if (cmd === 0x11) this.streaming = false;
    if ((cmd >= 0x38 && cmd <= 0x3f) || cmd === 0x10 || cmd === 0x11) return this.board.request(cmd, payload);
    if (cmd === 0x30) {
      // SET_DECIMATION: answered as applied (the synthetic data stays at 2 MS/s)
      const out = new Uint8Array(8);
      out[0] = payload[0] ?? 0;
      out[1] = payload[1] ?? 0;
      new DataView(out.buffer).setUint32(4, RATE >> (payload[0] ?? 0), true);
      return { ty: cmd | 0x40, status: 0, payload: out };
    }
    return { ty: cmd | 0x40, status: 0, payload: new Uint8Array() };
  }
  async setBuffer(samples: number) {
    this.pipeline.set_buffer_samples(samples);
  }
  async setCalibration(c: Calibration) {
    this.pipeline.set_calibration(c);
  }

  async setTrigger(c: TriggerCfg) {
    this.pipeline.set_trigger(c);
  }

  async events(since: number, max: number) {
    return this.pipeline.events_since(BigInt(since), max) as TrigEvent[];
  }

  async clearEvents() {
    this.pipeline.clear_events();
  }

  async analyze(t0: number, t1: number, req: AnalysisRequest) {
    return this.pipeline.analyze(t0, t1, req) as AnalysisResult;
  }
  async lod(t0: number, t1: number, bins: number) {
    return this.pipeline.lod(t0, t1, bins);
  }
  async stats(t0: number, t1: number) {
    return this.pipeline.stats(t0, t1) as RawSpanStats | null;
  }
  async valueAt(t: number) {
    return this.pipeline.value_at(t) as [number, number, number, number] | null;
  }

  async sessionLod(t0: number, t1: number, bins: number) {
    return this.pipeline.session_lod(t0, t1, bins);
  }

  async sessionStats(t0: number, t1: number) {
    return this.pipeline.session_stats(t0, t1) as RawSpanStats | null;
  }

  async setLogInterval(s: number) {
    this.pipeline.set_log_interval(s);
  }

  async clearSession() {
    this.pipeline.clear_session();
  }

  async exportPreview(cfg: ExportCfg, wallOffset: number, rows: number) {
    return this.pipeline.export_preview(cfg, wallOffset, rows);
  }

  /** what the demo device measures from now on */
  setScenario(s: DemoScenario) {
    this.gen.scenario = s;
  }

  /** the simulated target, on the 'target' scenario */
  readonly target = new DemoTarget(this.gen);
  get scenario(): DemoScenario {
    return this.gen.scenario;
  }

  async setMath(expr: string | null) {
    return this.pipeline.set_math(expr ?? undefined) ?? null;
  }
  async close() {
    for (const t of this.timers) clearInterval(t);
  }
}

/**
 * The demo's target: a microcontroller speaking SimpleSerial (ChipWhisperer's
 * protocol) whose current leaks what it computes, as a real one does.
 *
 *   k<key>       set the AES key (16 bytes)
 *   p<pt>        encrypt a block: replies r<ct>
 *   c<password>  check a password (up to 16 bytes, NUL-padded): replies r01 or r00
 *
 * Every command wakes it from 2 mA to ~6 mA for 100 µs, then:
 * - AES: 16 S-box lookups of 16 µs each, drawing 0.25 mA more per bit set in
 *   S(pt ⊕ key) (the classic first-round leak), started 0 to 8 µs late (an
 *   interrupt's jitter), then 300 µs of the other rounds.
 * - Password: a byte-by-byte compare that stops at the first wrong byte,
 *   25 µs per byte compared (a timing leak), then 50 µs to answer.
 */
export class DemoTarget {
  key = new Uint8Array([0x2b, 0x7e, 0x15, 0x16, 0x28, 0xae, 0xd2, 0xa6, 0xab, 0xf7, 0x15, 0x88, 0x09, 0xcf, 0x4f, 0x3c]);
  password = new TextEncoder().encode('nebulosa');

  constructor(private gen: Generator) {}

  /** a SimpleSerial command: the 'r' reply's bytes (empty when there is none) */
  async command(cmd: string, data: Uint8Array): Promise<Uint8Array> {
    const wake = (s: number) => 2e-3 + 4e-3 * Math.min(1, s / 20e-6);
    if (cmd === 'k') {
      this.key = data.slice(0, 16);
      await this.gen.schedule(50e-6, wake);
      return new Uint8Array();
    }
    if (cmd === 'p') {
      const pt = new Uint8Array(16);
      pt.set(data.subarray(0, 16));
      const leak = Array.from({ length: 16 }, (_, i) => hw(SBOX[pt[i] ^ this.key[i]]));
      const jitter = Math.floor(Math.random() * 3) * 0.5e-6; // 0..1 µs: alignment sharpens the leak, it shows without
      const sb = 100e-6 + jitter;
      await this.gen.schedule(sb + 16 * 16e-6 + 300e-6, (s) => {
        if (s < 100e-6) return wake(s);
        const k = Math.floor((s - sb) / 16e-6);
        if (s >= sb && k < 16) {
          const u = s - sb - k * 16e-6;
          return 6e-3 + (u >= 4e-6 && u < 12e-6 ? 0.5e-3 * leak[k] : 0.4e-3);
        }
        return 6e-3 + 0.6e-3 * Math.sin(s * 9e4) * Math.sin(s * 2.3e4);
      });
      return aesEncrypt(this.key, pt);
    }
    if (cmd === 'c') {
      let n = 0;
      while (n < this.password.length && n < data.length && data[n] === this.password[n]) n++;
      const ok = n === this.password.length && (data.length === n || data[n] === 0);
      const compared = Math.min(n + 1, this.password.length);
      // one clean pulse at 7 mA (well above any trigger level), its length the
      // byte-compare's duration: a right guess compares one more byte, so the
      // pulse is 40 µs longer — the timing leak.
      await this.gen.schedule(100e-6 + compared * 40e-6 + 50e-6, (s) => (s < 100e-6 ? wake(s) : 7e-3));
      return new Uint8Array([ok ? 1 : 0]);
    }
    throw new Error(`the demo target doesn't know command '${cmd}'`);
  }
}
