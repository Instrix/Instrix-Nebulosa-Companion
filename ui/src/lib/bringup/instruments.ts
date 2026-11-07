// Bench instruments for the AFE bring-up (Instrix-Nebulosa
// Design/afe1-x10-bringup.md): a Siglent SPD3303C power supply and a Korad
// KEL102 electronic load, both SCPI-like text. The desktop backend carries
// the lines (src-tauri/src/instruments.rs: /dev/usbtmcN, a serial port, or
// tcp:HOST[:PORT]); "sim" as the path gives a simulated one, a supply
// channel in series with the load (and the decade box), answering the same
// commands. (The Nebulosa Debugger's src/core/instruments.cpp.)

import { invoke } from '../ipc';
import { isNative } from '../device';

/** one line out, optionally one back */
interface Transport {
  io(line: string, reply: boolean, gapMs: number, timeoutMs: number): Promise<string | null>;
  close(): Promise<void>;
}

class NativeTransport implements Transport {
  constructor(private id: string) {}
  static async open(id: string, path: string, baud: number): Promise<NativeTransport> {
    await invoke<void>('inst_open', { id, path, baud });
    return new NativeTransport(id);
  }
  io(line: string, reply: boolean, gapMs: number, timeoutMs: number) {
    return invoke<string | null>('inst_io', { id: this.id, line, reply, gapMs, timeoutMs });
  }
  close() {
    return invoke<void>('inst_close', { id: this.id });
  }
}

export function listPorts(): Promise<{ path: string; label: string }[]> {
  return isNative ? invoke('inst_list') : Promise.resolve([]);
}

/** the first number in a reply ("1.2345A", "+5.000E+00", "CH1:5.00"); NaN if none */
export function parseNumber(s: string | null): number {
  const m = s?.match(/[-+]?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?/);
  return m ? parseFloat(m[0]) : NaN;
}

// ---- the simulated bench ----------------------------------------------------------------

/**
 * PSU CH1 (+) -> Nebulosa I+ ... I- -> load -> CH1 (-), a small burden in
 * the loop; CH2 into a fixed resistor (the Nebulosa on 12 V). With the
 * load's input off and a decade box set, the box is the loop.
 */
export class SimBench {
  vSet = [0, 0, 5.0];
  iSet = [0, 0, 3.2];
  out = [false, false, false];
  mode: 'CC' | 'CV' | 'CR' = 'CC';
  cc = 0;
  cr = 1000;
  cv = 0;
  input = false;
  rLoop = 0.02;
  /** the Nebulosa's burden on the tap that is on (shunt + switch), Ω */
  rTap = 0;
  rCh2 = 40;
  rBox = 0;
  private listeners = new Set<() => void>();

  onChange(f: () => void): () => void {
    this.listeners.add(f);
    return () => this.listeners.delete(f);
  }
  changed() {
    for (const f of this.listeners) f();
  }

  /** CH1 into the loop: the loop current, the load's voltage */
  solve(): { i: number; vLoad: number } {
    if (!this.out[0]) return { i: 0, vLoad: 0 };
    const vs = this.vSet[0];
    const rl = this.rLoop + this.rTap;
    let i = 0;
    if (!this.input && this.rBox > 0) i = vs / (this.rBox + rl);
    if (this.input) {
      if (this.mode === 'CC') i = this.cc;
      else if (this.mode === 'CR') i = vs / (this.cr + rl);
      else i = vs > this.cv ? (vs - this.cv) / rl : 0;
    }
    i = Math.min(i, this.iSet[0], vs / rl); // the supply's current limit
    return { i, vLoad: Math.max(0, vs - i * rl) };
  }

  /** a reading as the instrument would give it: a little noise, its resolution */
  private read(x: number, res: number) {
    return Math.round((x + (Math.random() - 0.5) * res) / res) * res;
  }

  psu(line: string): string | null {
    const l = line.trim().toUpperCase();
    let m: RegExpMatchArray | null;
    if (l === '*IDN?') return 'Siglent Technologies,SPD3303C,SIMULATED,1.0';
    if ((m = l.match(/^CH([12]):VOLT\s+([\d.]+)/))) this.vSet[+m[1] - 1] = +m[2];
    else if ((m = l.match(/^CH([12]):CURR\s+([\d.]+)/))) this.iSet[+m[1] - 1] = +m[2];
    else if ((m = l.match(/^OUTP\s+CH([123]),(ON|OFF)/))) this.out[+m[1] - 1] = m[2] === 'ON';
    else if ((m = l.match(/^MEAS:(VOLT|CURR)\?\s*CH([12])/))) {
      const ch = +m[2] - 1;
      let v = 0,
        a = 0;
      if (ch === 0) {
        const s = this.solve();
        v = this.out[0] ? this.vSet[0] : 0;
        a = s.i;
      } else if (this.out[1]) {
        v = this.vSet[1];
        a = Math.min(this.iSet[1], v / this.rCh2);
      }
      return m[1] === 'VOLT' ? this.read(v, 0.001).toFixed(3) : this.read(a, 0.001).toFixed(3);
    }
    this.changed();
    return null;
  }

  load(line: string): string | null {
    const l = line.trim().toUpperCase();
    let m: RegExpMatchArray | null;
    if (l === '*IDN?') return 'KORAD KEL102 SIMULATED';
    if ((m = l.match(/^:FUNC\s+(CC|CV|CR)/))) this.mode = m[1] as 'CC';
    else if ((m = l.match(/^:CURR\s+([\d.]+)/))) this.cc = +m[1];
    else if ((m = l.match(/^:RES\s+([\d.]+)/))) this.cr = +m[1];
    else if ((m = l.match(/^:VOLT\s+([\d.]+)/))) this.cv = +m[1];
    else if ((m = l.match(/^:INP\s+(ON|OFF)/))) this.input = m[1] === 'ON';
    else if (l === ':MEAS:VOLT?') {
      // input off, box mode: the load reads the voltage across CH1
      const s = this.solve();
      return `${this.read(this.input ? s.vLoad : this.out[0] ? this.vSet[0] : 0, 1e-4).toFixed(4)}V`;
    } else if (l === ':MEAS:CURR?') {
      const s = this.solve();
      return `${this.read(this.input ? s.i : 0, 1e-4).toFixed(4)}A`;
    }
    this.changed();
    return null;
  }
}

/** one simulated bench, shared by the simulated supply and load */
export const simBench = new SimBench();

class SimTransport implements Transport {
  constructor(private answer: (line: string) => string | null) {}
  async io(line: string, reply: boolean) {
    await new Promise((r) => setTimeout(r, 4));
    const a = this.answer(line);
    return reply ? (a ?? '') : null;
  }
  async close() {}
}

// ---- the drivers ----------------------------------------------------------------------

abstract class Instrument {
  protected port: Transport | null = null;
  path = '';
  idn = '';
  /** compound operations (set V then I) stay together */
  private queue: Promise<unknown> = Promise.resolve();

  get open(): boolean {
    return !!this.port;
  }
  get simulated(): boolean {
    return this.path === 'sim';
  }

  protected abstract gap: number;
  protected abstract timeout: number;
  protected abstract simAnswer: (line: string) => string | null;

  async connect(id: string, path: string, baud: number) {
    await this.disconnect();
    this.path = path;
    this.port = path === 'sim' ? new SimTransport(this.simAnswer) : await NativeTransport.open(id, path, baud);
  }

  async disconnect() {
    const p = this.port;
    this.port = null;
    this.idn = '';
    await p?.close().catch(() => undefined);
  }

  protected run<T>(f: (p: Transport) => Promise<T>): Promise<T> {
    const go = () => {
      if (!this.port) throw new Error('not open');
      return f(this.port);
    };
    const r = this.queue.then(go, go);
    this.queue = r.catch(() => undefined);
    return r;
  }

  protected cmd(p: Transport, line: string) {
    return p.io(line, false, this.gap, this.timeout);
  }
  protected async query(p: Transport, line: string): Promise<string> {
    return (await p.io(line, true, Math.floor(this.gap / 2), this.timeout)) ?? '';
  }

  identify(): Promise<string> {
    return this.run(async (p) => (this.idn = await this.query(p, '*IDN?')));
  }
}

/** Siglent SPD3303C (CH1, CH2; CH3 fixed 2.5 / 3.3 / 5 V) */
export class Psu extends Instrument {
  protected gap = 60; // the SPD3303 loses commands sent back to back
  protected timeout = 2000;
  protected simAnswer = (l: string) => simBench.psu(l);

  /** OUTP:TRACK 0: CH1 and CH2 isolated from each other */
  independent() {
    return this.run((p) => this.cmd(p, 'OUTP:TRACK 0'));
  }
  set(ch: number, volts: number, amps: number) {
    return this.run(async (p) => {
      await this.cmd(p, `CH${ch}:VOLT ${volts.toFixed(3)}`);
      await this.cmd(p, `CH${ch}:CURR ${amps.toFixed(3)}`);
    });
  }
  output(ch: number, on: boolean) {
    return this.run((p) => this.cmd(p, `OUTP CH${ch},${on ? 'ON' : 'OFF'}`));
  }
  measure(ch: number): Promise<{ v: number; a: number }> {
    return this.run(async (p) => ({ v: parseNumber(await this.query(p, `MEAS:VOLT? CH${ch}`)), a: parseNumber(await this.query(p, `MEAS:CURR? CH${ch}`)) }));
  }
}

/** Korad KEL102 electronic load */
export class Load extends Instrument {
  protected gap = 30;
  protected timeout = 1000;
  protected simAnswer = (l: string) => simBench.load(l);

  mode(m: 'CC' | 'CV' | 'CR') {
    return this.run((p) => this.cmd(p, `:FUNC ${m}`));
  }
  setCurrent(a: number) {
    return this.run((p) => this.cmd(p, `:CURR ${a.toFixed(4)}A`));
  }
  setResistance(ohm: number) {
    return this.run((p) => this.cmd(p, `:RES ${ohm.toFixed(4)}OHM`));
  }
  input(on: boolean) {
    return this.run((p) => this.cmd(p, on ? ':INP ON' : ':INP OFF'));
  }
  measure(): Promise<{ v: number; a: number }> {
    return this.run(async (p) => ({ v: parseNumber(await this.query(p, ':MEAS:VOLT?')), a: parseNumber(await this.query(p, ':MEAS:CURR?')) }));
  }
}
