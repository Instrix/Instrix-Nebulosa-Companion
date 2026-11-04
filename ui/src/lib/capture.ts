// Driving a side-channel capture campaign: for each trace, give the target an
// input, wait for the trigger to fire on the operation, and cut the samples
// out of the buffer at the trigger into the campaign (sca.ts / sca.rs).
//
// The trigger is a rising edge on the chosen signal: its event time is the
// edge itself, so a trace starts there. A real external trigger (a GPIO the
// target raises) fits the same way once the gateware routes it into a sample.

import type { Link } from './device';
import { SBOX, aesEncrypt, fromHex, hw } from './aes';
import type { Target } from './targets';

export type Plan =
  | { kind: 'aes-random' } // a random plaintext each trace (for correlation / overview)
  | { kind: 'aes-tvla'; fixed: Uint8Array } // half fixed plaintext, half random (TVLA t-test)
  | { kind: 'password'; guesses: string[] }; // try each password guess in turn, repeated

export interface CaptureOpts {
  link: Link;
  target: Target;
  /** samples per trace */
  samples: number;
  /** signal the trigger watches and the campaign records */
  source: 'i' | 'v' | 'p' | 'm';
  plan: Plan;
  traces: number;
  /** the trigger kind set for this run: 'edge' (event at the rising edge) or
   *  'pulse' (event at the falling edge, width = the op's duration) */
  triggerKind: 'edge' | 'pulse';
  /** s per sample (1 / rate) */
  dt: number;
  /** ms to wait for the trigger after sending the input */
  timeoutMs?: number;
  /** ms to let the stream settle after the trigger, before cutting the trace */
  settleMs?: number;
  /** after each trace: the count so far, and the last trace's peek */
  onProgress?: (done: number, info: TraceInfo) => void;
  /** keep going while this returns true */
  running?: () => boolean;
}

export interface TraceInfo {
  group: number;
  input: Uint8Array;
  output: Uint8Array;
  /** s between the input and the trigger */
  latency: number;
}

const rand16 = () => Uint8Array.from({ length: 16 }, () => Math.floor(Math.random() * 256));

/** the input and group for trace k of a plan */
function inputFor(plan: Plan, k: number): { cmd: string; data: Uint8Array; group: number } {
  switch (plan.kind) {
    case 'aes-random':
      return { cmd: 'p', data: rand16(), group: 0 };
    case 'aes-tvla': {
      const fixed = k % 2 === 0;
      return { cmd: 'p', data: fixed ? plan.fixed : rand16(), group: fixed ? 0 : 1 };
    }
    case 'password': {
      const g = plan.guesses[k % plan.guesses.length];
      const data = new Uint8Array(16);
      data.set(new TextEncoder().encode(g).subarray(0, 16));
      return { cmd: 'c', data, group: k % plan.guesses.length };
    }
  }
}

/**
 * Run a campaign on the engine behind `link`. Returns how many traces it
 * added. Stops on `running() === false`, when `traces` is reached, or on an
 * error after too many misses in a row.
 */
export async function runCapture(o: CaptureOpts): Promise<number> {
  const { link, target, plan } = o;
  const timeout = o.timeoutMs ?? 500;
  await link.sca.create({ samples: o.samples, source: o.source });

  let added = 0;
  let misses = 0;
  // the trigger's event count now: wait for it to grow after each input
  let seq = (await link.events(0, 1_000_000_000)).at(-1)?.seq ?? 0;
  const since = await link.events(0, 1);
  let next = since.length ? seq + 1 : 0;

  for (let k = 0; o.running?.() !== false && added < o.traces; k++) {
    const { cmd, data, group } = inputFor(plan, k);
    const t0 = performance.now();
    let output: Uint8Array;
    try {
      output = await target.command(cmd, data, 2000);
    } catch (e) {
      throw new Error(`the target didn't answer: ${e instanceof Error ? e.message : e}`);
    }

    // the trigger event the operation raised. Wait for it, then let the stream
    // settle a little so the operation's whole tail is in the buffer, and take
    // the most recent event — the one this input just caused.
    if (!(await waitEvent(link, next, timeout))) {
      if (++misses > 20) throw new Error('the trigger never fired — check its source and level (it must cross on the operation)');
      continue;
    }
    await new Promise((r) => setTimeout(r, o.settleMs ?? 60));
    const ev = (await link.events(next, 1000)).at(-1)!;
    misses = 0;
    next = ev.seq + 1;

    // a pulse event sits at the falling edge: back up by its width to the start
    const width = ev.width ?? 0;
    const start = o.triggerKind === 'pulse' ? ev.t - width : ev.t;
    try {
      added = await link.sca.add(start, start + o.samples * o.dt, { input: [...data], output: [...output], group, width });
    } catch (e) {
      // the samples slid out, or a gap: skip this one
      if (++misses > 20) throw new Error(`can't cut the trace out: ${e instanceof Error ? e.message : e}`);
      continue;
    }
    o.onProgress?.(added, { group, input: data, output, latency: (performance.now() - t0) / 1000 });
  }
  return added;
}

/** the first trigger event with seq ≥ `seq`, or null after `timeoutMs` */
async function waitEvent(link: Link, seq: number, timeoutMs: number) {
  const deadline = performance.now() + timeoutMs;
  for (;;) {
    const evs = await link.events(seq, 4);
    if (evs.length) return evs[0];
    if (performance.now() > deadline) return null;
    await new Promise((r) => setTimeout(r, 10));
  }
}

/** parse a hex key box; the default AES-128 test key otherwise */
export function keyOrDefault(s: string): Uint8Array {
  return fromHex(s) ?? new Uint8Array([0x2b, 0x7e, 0x15, 0x16, 0x28, 0xae, 0xd2, 0xa6, 0xab, 0xf7, 0x15, 0x88, 0x09, 0xcf, 0x4f, 0x3c]);
}

// Re-exported so the tab (and later a CPA view) share one leakage model.
export { SBOX, aesEncrypt, hw };
