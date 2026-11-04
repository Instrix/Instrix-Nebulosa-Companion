// The serial monitor: a configurable serial terminal that runs alongside the
// logger. Its received lines become timed markers on the power log; it can
// send, toggle RTS / DTR, and raise a special marker when a line matches a
// pattern. Web Serial in the browser, a native serial port in the desktop app.

import { isNative } from './device';
import type { Annotation } from './LoggerView.svelte';

export interface SerialConfig {
  baud: number;
  /** 5, 6, 7 or 8 */
  dataBits: 5 | 6 | 7 | 8;
  parity: 'none' | 'even' | 'odd';
  stopBits: 1 | 2;
  flow: 'none' | 'hardware' | 'software';
  /** control lines asserted on open */
  rts: boolean;
  dtr: boolean;
  /** how received bytes are split into lines */
  rxNewline: 'lf' | 'cr' | 'crlf' | 'any';
  /** appended to what you send */
  txNewline: 'none' | 'lf' | 'cr' | 'crlf';
  /** show what you send in the monitor too */
  echo: boolean;
  /** raise a highlighted marker when a received line matches */
  trigger: {
    on: boolean;
    pattern: string;
    /** treat the pattern as a regular expression */
    regex: boolean;
    /** also stop the logger following, to freeze the match on screen */
    hold: boolean;
  };
}

export const defaultSerialConfig: SerialConfig = {
  baud: 115200,
  dataBits: 8,
  parity: 'none',
  stopBits: 1,
  flow: 'none',
  rts: false,
  dtr: false,
  rxNewline: 'any',
  txNewline: 'lf',
  echo: true,
  trigger: { on: false, pattern: '', regex: false, hold: false },
};

const EOL: Record<SerialConfig['txNewline'], string> = { none: '', lf: '\n', cr: '\r', crlf: '\r\n' };

export interface MonitorHandle {
  readonly name: string;
  write(bytes: Uint8Array): Promise<void>;
  signals(rts: boolean, dtr: boolean): Promise<void>;
  close(): Promise<void>;
}

/** split a growing buffer into lines per the rx rule; returns [lines, rest] */
function splitLines(buf: string, mode: SerialConfig['rxNewline']): [string[], string] {
  const re = mode === 'lf' ? /\n/ : mode === 'cr' ? /\r/ : mode === 'crlf' ? /\r\n/ : /\r\n|\r|\n/;
  const parts = buf.split(re);
  const rest = parts.pop() ?? '';
  return [parts, rest];
}

async function openPort(cfg: SerialConfig, onBytes: (b: Uint8Array) => void, path?: string): Promise<MonitorHandle> {
  if (isNative) {
    const { Channel, invoke } = await import('@tauri-apps/api/core');
    const ch = new Channel<number[]>();
    ch.onmessage = (b) => onBytes(Uint8Array.from(b));
    const portCfg = {
      baud: cfg.baud,
      data_bits: cfg.dataBits,
      parity: cfg.parity,
      stop_bits: cfg.stopBits,
      flow: cfg.flow,
      rts: cfg.rts,
      dtr: cfg.dtr,
    };
    await invoke('serial_open', { path, cfg: portCfg, onData: ch });
    return {
      name: `${path} · ${cfg.baud} ${cfg.dataBits}${cfg.parity[0].toUpperCase()}${cfg.stopBits}`,
      write: (bytes) => invoke('serial_write', { data: Array.from(bytes) }),
      signals: (rts, dtr) => invoke('serial_signals', { rts, dtr }),
      close: () => invoke('serial_close'),
    };
  }
  const serial = (navigator as unknown as { serial?: { requestPort(): Promise<WebPort> } }).serial;
  if (!serial) throw new Error('This browser has no Web Serial: use Chrome or Edge, or the desktop app.');
  const port = await serial.requestPort();
  await port.open({
    baudRate: cfg.baud,
    dataBits: cfg.dataBits,
    stopBits: cfg.stopBits,
    parity: cfg.parity,
    flowControl: cfg.flow === 'hardware' ? 'hardware' : 'none',
    bufferSize: 4096,
  });
  await port.setSignals?.({ requestToSend: cfg.rts, dataTerminalReady: cfg.dtr }).catch(() => undefined);
  const reader = port.readable.getReader();
  const writer = port.writable.getWriter();
  let open = true;
  (async () => {
    while (open) {
      const { value, done } = await reader.read().catch(() => ({ value: undefined, done: true }));
      if (done) break;
      if (value) onBytes(value);
    }
  })();
  return {
    name: `Serial ${cfg.baud} ${cfg.dataBits}${cfg.parity[0].toUpperCase()}${cfg.stopBits}`,
    write: (bytes) => writer.write(bytes),
    signals: (rts, dtr) => port.setSignals?.({ requestToSend: rts, dataTerminalReady: dtr }) ?? Promise.resolve(),
    close: async () => {
      open = false;
      await reader.cancel().catch(() => undefined);
      reader.releaseLock();
      await writer.close().catch(() => undefined);
      await port.close().catch(() => undefined);
    },
  };
}

interface WebPort {
  open(o: object): Promise<void>;
  close(): Promise<void>;
  setSignals?(s: { requestToSend?: boolean; dataTerminalReady?: boolean }): Promise<void>;
  readable: ReadableStream<Uint8Array>;
  writable: WritableStream<Uint8Array>;
}

export class SerialMonitor {
  annotations = $state<Annotation[]>([]);
  handle = $state.raw<MonitorHandle | null>(null);
  error = $state('');
  /** how many received lines have matched the trigger */
  matches = $state(0);
  readonly cap = 5000;

  private rxBuf = '';
  private dec = new TextDecoder();

  constructor(
    /** the current measurement time (FPGA s) for a new line */
    private now: () => number,
    private cfg: () => SerialConfig,
    /** called when a received line matches the trigger (for a logger hold, etc.) */
    private onTrigger?: (a: Annotation) => void,
  ) {}

  get open() {
    return !!this.handle;
  }

  async connect(path?: string) {
    this.error = '';
    try {
      this.rxBuf = '';
      this.handle = await openPort(this.cfg(), (b) => this.onBytes(b), path);
    } catch (e) {
      this.error = String(e instanceof Error ? e.message : e);
    }
  }

  private onBytes(b: Uint8Array) {
    this.rxBuf += this.dec.decode(b, { stream: true });
    const [lines, rest] = splitLines(this.rxBuf, this.cfg().rxNewline);
    this.rxBuf = rest;
    for (const line of lines) if (line.length) this.rx(line);
  }

  private matched(text: string): boolean {
    const t = this.cfg().trigger;
    if (!t.on || !t.pattern) return false;
    try {
      return t.regex ? new RegExp(t.pattern).test(text) : text.includes(t.pattern);
    } catch {
      return false; // a half-typed regex
    }
  }

  private rx(text: string) {
    const hit = this.matched(text);
    const a: Annotation = { t: this.now(), text, wall: Date.now(), kind: hit ? 'trigger' : 'rx' };
    this.push(a);
    if (hit) {
      this.matches++;
      this.onTrigger?.(a);
    }
  }

  /** send text (with the configured line ending); echoed as a tx marker */
  async send(text: string) {
    if (!this.handle) return;
    const bytes = new TextEncoder().encode(text + EOL[this.cfg().txNewline]);
    try {
      await this.handle.write(bytes);
      if (this.cfg().echo) this.push({ t: this.now(), text: `» ${text}`, wall: Date.now(), kind: 'tx' });
    } catch (e) {
      this.error = String(e);
    }
  }

  async setSignals(rts: boolean, dtr: boolean) {
    await this.handle?.signals(rts, dtr).catch((e) => (this.error = String(e)));
  }

  /** add a message by hand (the simultaneous-serial demo); trigger rules apply */
  note(text: string) {
    this.rx(text);
  }

  private push(a: Annotation) {
    this.annotations = [...this.annotations.slice(-(this.cap - 1)), a];
  }

  clear() {
    this.annotations = [];
    this.matches = 0;
  }

  async disconnect() {
    await this.handle?.close().catch(() => undefined);
    this.handle = null;
  }
}
