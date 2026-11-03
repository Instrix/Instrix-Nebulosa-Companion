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
import { core, type AnalysisRequest, type AnalysisResult, type Calibration, type Meta, type RawSpanStats, type Reply, type TrigEvent, type TriggerCfg } from './nbp';

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

/** the smallest range whose full scale holds `a` with some margin */
function rangeFor(a: number) {
  for (let r = RANGE_FS.length - 1; r >= 0; r--) if (a < RANGE_FS[r] * 0.9) return r;
  return 0;
}

class Generator {
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
      if (this.ts % (RATE * 2.7) < 1 && this.ts > 0) {
        this.ts += RATE * 0.015;
        this.range = -1; // a new block after the gap
      }
      const phase = t % 0.5;
      const burst = phase < 0.04;
      const base = burst ? 25e-3 + 3e-3 * Math.sin(phase * 900) : 200e-6 + 20e-6 * Math.sin(t * 7);
      const i = Math.max(0, base * (1 + (Math.random() - 0.5) * (burst ? 0.08 : 0.15)));
      const v = 3.3 - i * 0.8 + (Math.random() - 0.5) * 2e-3;
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

/** A Link to the synthetic device; commands are accepted and ignored. */
export class DemoLink implements Link {
  readonly description = 'Demo device (synthetic, 2 MS/s)';
  private pipeline = new core.Pipeline();
  private gen = new Generator();
  private timers: ReturnType<typeof setInterval>[] = [];

  constructor(ev: LinkEvents) {
    let sent = -1;
    this.timers.push(
      setInterval(() => {
        for (const m of this.gen.next((RATE * TICK_MS) / 1000)) this.pipeline.push(0, m);
        const meta = this.pipeline.meta() as Meta;
        if (meta.version !== sent) {
          sent = meta.version;
          ev.onMeta(meta);
        }
      }, TICK_MS),
    );
  }

  async request(cmd: number): Promise<Reply> {
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

  async setMath(expr: string | null) {
    return this.pipeline.set_math(expr ?? undefined) ?? null;
  }
  async close() {
    for (const t of this.timers) clearInterval(t);
  }
}
