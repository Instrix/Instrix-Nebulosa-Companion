// The device under test, as the side-channel tab drives it: SimpleSerial
// (ChipWhisperer's protocol, v1) over a serial port, or the demo device's
// simulated target.
//
// SimpleSerial v1: the host sends `<cmd><hex>\n`; the target answers
// `r<hex>\n` with a result, then `z<status>\n` (z00: done).

import { DemoTarget } from './demo';
import { toHex } from './aes';

export interface Target {
  readonly name: string;
  /** a command and its data: the 'r' reply's bytes (empty when it has none) */
  command(cmd: string, data: Uint8Array, timeoutMs?: number): Promise<Uint8Array>;
  close(): Promise<void>;
}

const withTimeout = <T>(p: Promise<T>, ms: number, what: string) =>
  Promise.race([p, new Promise<T>((_, no) => setTimeout(() => no(new Error(`${what}: no answer in ${ms} ms`)), ms))]);

export function demoTarget(t: DemoTarget): Target {
  return {
    name: 'Demo target (leaky AES, password check)',
    command: (cmd, data, timeoutMs = 2000) => withTimeout(t.command(cmd, data), timeoutMs, 'the demo target (is the stream running?)'),
    close: async () => undefined,
  };
}

/** SimpleSerial over a byte stream: `write` sends, lines come from `read` */
export class SimpleSerial implements Target {
  private buf = '';
  private waiters: ((line: string) => void)[] = [];
  private lines: string[] = [];

  constructor(
    readonly name: string,
    private write: (b: Uint8Array) => Promise<void>,
    private closePort: () => Promise<void>,
  ) {}

  /** bytes from the port */
  feed(b: Uint8Array) {
    this.buf += new TextDecoder().decode(b);
    let k;
    while ((k = this.buf.indexOf('\n')) >= 0) {
      const line = this.buf.slice(0, k).replace(/\r$/, '');
      this.buf = this.buf.slice(k + 1);
      if (!line) continue;
      const w = this.waiters.shift();
      if (w) w(line);
      else this.lines.push(line);
    }
  }

  private nextLine(): Promise<string> {
    const l = this.lines.shift();
    return l != null ? Promise.resolve(l) : new Promise((r) => this.waiters.push(r));
  }

  async command(cmd: string, data: Uint8Array, timeoutMs = 1000): Promise<Uint8Array> {
    this.lines = []; // whatever came unasked
    await this.write(new TextEncoder().encode(`${cmd}${toHex(data)}\n`));
    const run = async () => {
      let out = new Uint8Array();
      for (;;) {
        const line = await this.nextLine();
        if (line[0] === 'r') out = Uint8Array.from(line.slice(1).match(/../g) ?? [], (h) => parseInt(h, 16));
        else if (line[0] === 'z') {
          const st = parseInt(line.slice(1), 16);
          if (st) throw new Error(`the target answered error ${line.slice(1)}`);
          return out;
        } else if (line[0] === 'e') throw new Error(`the target: ${line}`);
      }
    };
    return withTimeout(run(), timeoutMs, `SimpleSerial '${cmd}'`);
  }

  close() {
    return this.closePort();
  }
}

/** Web Serial (Chromium): asks the user for a port */
export async function webSerialTarget(baud: number): Promise<Target> {
  const serial = (navigator as unknown as { serial?: WebSerial }).serial;
  if (!serial) throw new Error('This browser has no Web Serial: use Chrome or Edge, or the desktop app.');
  const port = await serial.requestPort();
  await port.open({ baudRate: baud });
  const writer = port.writable.getWriter();
  const reader = port.readable.getReader();
  let open = true;
  const ss = new SimpleSerial(
    `Serial ${baud} Bd`,
    (b) => writer.write(b),
    async () => {
      open = false;
      await reader.cancel().catch(() => undefined);
      reader.releaseLock();
      writer.releaseLock();
      await port.close().catch(() => undefined);
    },
  );
  (async () => {
    while (open) {
      const { value, done } = await reader.read().catch(() => ({ value: undefined, done: true }));
      if (done) break;
      if (value) ss.feed(value);
    }
  })();
  return ss;
}

interface WebSerialPort {
  open(o: { baudRate: number }): Promise<void>;
  close(): Promise<void>;
  readable: ReadableStream<Uint8Array>;
  writable: WritableStream<Uint8Array>;
}
interface WebSerial {
  requestPort(): Promise<WebSerialPort>;
}

/** the desktop app's serial ports */
export async function nativeSerialPorts(): Promise<{ path: string; label: string }[]> {
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke('serial_list');
}

/** SimpleSerial on a serial port, in the desktop app */
export async function nativeSerialTarget(path: string, baud: number): Promise<Target> {
  const { Channel, invoke } = await import('@tauri-apps/api/core');
  const ch = new Channel<number[]>();
  const ss = new SimpleSerial(
    `${path} · ${baud} Bd`,
    (b) => invoke('serial_write', { data: Array.from(b) }),
    () => invoke('serial_close'),
  );
  ch.onmessage = (b) => ss.feed(Uint8Array.from(b));
  await invoke('serial_open', { path, cfg: { baud, data_bits: 8, parity: 'none', stop_bits: 1, flow: 'none', rts: false, dtr: false }, onData: ch });
  return ss;
}
