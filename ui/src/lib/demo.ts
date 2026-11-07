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
import { core, type Reply } from './nbp';
import { aesEncrypt, hw, SBOX } from './aes';
import { CoreLink } from './coreLink';
import { RATE, type DemoScenario, type OpSpec } from './demoGen';

export type { DemoScenario } from './demoGen';

export const demoRequested = typeof location !== 'undefined' && new URLSearchParams(location.search).has('demo');

/** `sel=a,b`: seconds before the newest sample */
export function demoSelection(): [number, number] | null {
  const s = new URLSearchParams(location.search).get('sel');
  if (!s) return null;
  const [a, b] = s.split(',').map(Number);
  return isFinite(a) && isFinite(b) ? [a, b] : null;
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

/**
 * A Link to the synthetic device; most commands are accepted and ignored. The
 * generator and the pipeline run in the core host (a worker): the UI only
 * gets Meta and answers to its queries.
 */
/**
 * A simulated FPGA behind the demo's registers (for the Debug tab): the
 * register map (ID 0x4E42000B, 100 MHz), the range switch's state, timing
 * and forced comparators, and an RS1473 link that locks at some sample
 * phases and loses frames at the others.
 */
class FakeFpga {
  regs = new Uint32Array(45);
  t0 = performance.now();
  /** phases where each isolator group reads settled bits */
  good = [0b01110, 0b11100];
  /** RANGE_STATUS 29:24, the raw comparator pins */
  pins = 0;
  /** where the lines change (the eyes' edges) */
  maps = [0b10001, 0b00011];
  constructor() {
    const r = this.regs;
    r[34] = 0x4e42000b;
    r[41] = 100_000_000;
    r[31] = 2; // the ADCs
    r[37] = 1 | (0 << 8) | (5 << 12); // automatic 0..5
    r[38] = 2 | (16 << 6) | (26 << 12) | (27 << 20);
    r[40] = 40 | (1 << 16) | (1 << 17) | (1 << 18);
    r[43] = 7 | (8 << 8);
    this.setRange(3);
    this.relock();
  }
  setRange(k: number) {
    const r = this.regs;
    r[35] = k | (1 << (8 + k)) | (r[35] & (0x3f << 16));
    r[36]++;
  }
  /** ADC_TIMING from ADC_CTRL: the measured phases (the middle of each eye), or the fixed ones */
  relock() {
    const c = this.regs[31];
    const manual = (c & (1 << 4)) !== 0;
    let t = manual ? 1 << 31 : 0;
    for (let k = 0; k < 2; k++) {
      const p = manual ? (c >> (5 + 3 * k)) & 7 : k ? 3 : 2;
      const ok = ((this.good[k] >> p) & 1) !== 0;
      const tmap = this.maps[k];
      t |= (p | (ok ? 1 << 3 : 0) | (tmap << 4) | (2 << 9)) << (16 * k);
      if (!ok) this.regs[44] += 37;
    }
    this.regs[42] = t >>> 0;
    const echo = (c & (1 << 11)) !== 0;
    this.regs[43] = echo ? 0x8080 | (0b00110 << 16) | (0b01100 << 24) : 7 | (8 << 8);
  }
  request(cmd: number, p: Uint8Array): Uint8Array | null {
    const dv = new DataView(p.buffer, p.byteOffset, p.byteLength);
    const r = this.regs;
    const u32 = (...v: number[]) => {
      const o = new Uint8Array(4 * v.length);
      const d = new DataView(o.buffer);
      v.forEach((x, i) => d.setUint32(4 * i, x >>> 0, true));
      return o;
    };
    switch (cmd) {
      case 0x01: // PING
        return p.slice();
      case 0x03: {
        // REG_READ
        const i = p[0] ?? 0;
        if (i >= r.length) return null;
        if (i === 32) r[32] = (Math.random() * 64) | 0;
        if (r[31] & (1 << 4) && i === 44) this.relock(); // a bad fixed phase keeps losing frames
        const o = new Uint8Array(8);
        o[0] = i;
        new DataView(o.buffer).setUint32(4, r[i], true);
        return o;
      }
      case 0x04: {
        // REG_WRITE
        const i = p[0] ?? 0;
        if (i >= r.length || p.length < 8) return null;
        r[i] = dv.getUint32(4, true);
        if (i === 31) this.relock();
        return new Uint8Array();
      }
      case 0x12: // GET_STATS
        return u32(performance.now() - this.t0, 2, 0, 0, 0, r[33], r[32], r[35], r[39], r[31], 0, 0, 0, 0, 0, 0);
      case 0x20: // SET_SOURCE: bits 0-1 of ADC_CTRL
        r[31] = (r[31] & ~3) | ((p[0] ?? 0) & 3);
        return new Uint8Array();
      case 0x21: {
        // SET_RANGE (clears the force bits)
        const [auto, man, lo, hi] = p;
        r[37] = (auto ? 1 : 0) | ((man & 7) << 4) | ((lo & 7) << 8) | ((hi & 7) << 12);
        r[35] &= ~(0x3f << 16);
        if (!auto) this.setRange(Math.min(5, man));
        return new Uint8Array();
      }
      case 0x22: // GET_RANGE (the comparator pins from the bring-up's simulated bench)
        return u32((r[35] & ~(0x3f << 24)) | (this.pins << 24), r[36], r[37], r[38]);
      case 0x23: // SET_RANGE_TIMING
        r[38] = (p[0] & 0x3f) | ((p[1] & 0x3f) << 6) | (p[2] << 12) | ((dv.getUint16(4, true) & 0xfff) << 20);
        return new Uint8Array();
      case 0x24: {
        // SET_RANGE_FORCE: automatic mode steps one range per forced UP / DOWN
        r[37] = p[0] ? r[37] | 2 : r[37] & ~2;
        r[35] = (r[35] & ~(0x3f << 16)) | ((p[0] ? p[1] & 0x3f : 0) << 16);
        if (p[0] && r[37] & 1) {
          const up = (p[1] & 0b010101) !== 0,
            down = (p[1] & 0b101010) !== 0;
          const k = r[35] & 7;
          const lo = (r[37] >> 8) & 7,
            hi = (r[37] >> 12) & 7;
          if (up && !down && k > lo) this.setRange(k - 1);
          else if (down && !up && k < hi) this.setRange(k + 1);
        }
        return new Uint8Array();
      }
    }
    return null;
  }
}

export class DemoLink extends CoreLink implements Link {
  readonly description = 'Demo device (synthetic, 2 MS/s)';
  private board = new FakeBoard();
  private fpga = new FakeFpga();
  private scenarioNow: DemoScenario = 'iot';

  constructor(ev: LinkEvents) {
    super(ev);
    void this.call('demo.start');
  }

  async request(cmd: number, payload = new Uint8Array()): Promise<Reply> {
    // STREAM_START / STREAM_STOP resume and pause the data (it starts streaming, like a connected board)
    if (cmd === 0x10) void this.call('demo.stream', true);
    if (cmd === 0x11) void this.call('demo.stream', false);
    if ((cmd >= 0x38 && cmd <= 0x3f) || cmd === 0x10 || cmd === 0x11) return this.board.request(cmd, payload);
    if (cmd === 0x30) {
      // SET_DECIMATION: answered as applied (the synthetic data stays at 2 MS/s)
      const out = new Uint8Array(8);
      out[0] = payload[0] ?? 0;
      out[1] = payload[1] ?? 0;
      new DataView(out.buffer).setUint32(4, RATE >> (payload[0] ?? 0), true);
      return { ty: cmd | 0x40, status: 0, payload: out };
    }
    const f = this.fpga.request(cmd, payload);
    if (f) return { ty: cmd | 0x40, status: 0, payload: f };
    if (cmd === 0x03 || cmd === 0x04) return { ty: cmd | 0x40, status: 3, payload: new Uint8Array() }; // BAD_ARG
    return { ty: cmd | 0x40, status: 0, payload: new Uint8Array() };
  }

  /** the bring-up's simulated bench: its loop current as raw codes on `range`, the comparator pins (null: back to the scenario) */
  setBench(b: { range: number; code: number; pins: number } | null) {
    this.fpga.pins = b?.pins ?? 0;
    void this.call('demo.bench', b ? { range: b.range, code: b.code } : null);
  }

  /** what the demo device measures from now on */
  setScenario(s: DemoScenario) {
    this.scenarioNow = s;
    void this.call('demo.scenario', s);
  }
  get scenario(): DemoScenario {
    return this.scenarioNow;
  }

  /** the simulated target, on the 'target' scenario: its work runs in the generator's time */
  readonly target = new DemoTarget((dur, op) => this.call<void>('demo.schedule', dur, op));

  async close() {
    await this.closeCore();
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

  constructor(private schedule: (dur: number, op: OpSpec) => Promise<void>) {}

  /** a SimpleSerial command: the 'r' reply's bytes (empty when there is none) */
  async command(cmd: string, data: Uint8Array): Promise<Uint8Array> {
    if (cmd === 'k') {
      this.key = data.slice(0, 16);
      await this.schedule(50e-6, { kind: 'wake' });
      return new Uint8Array();
    }
    if (cmd === 'p') {
      const pt = new Uint8Array(16);
      pt.set(data.subarray(0, 16));
      const leak = Array.from({ length: 16 }, (_, i) => hw(SBOX[pt[i] ^ this.key[i]]));
      const jitter = Math.floor(Math.random() * 3) * 0.5e-6; // 0..1 µs: alignment sharpens the leak, it shows without
      const sb = 100e-6 + jitter;
      await this.schedule(sb + 16 * 16e-6 + 300e-6, { kind: 'aes', sb, leak });
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
      await this.schedule(100e-6 + compared * 40e-6 + 50e-6, { kind: 'pulse', after: 100e-6, level: 7e-3 });
      return new Uint8Array([ok ? 1 : 0]);
    }
    throw new Error(`the demo target doesn't know command '${cmd}'`);
  }
}
