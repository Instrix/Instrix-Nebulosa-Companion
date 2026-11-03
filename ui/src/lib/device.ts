// One connected device, whatever the transport. The native build (Tauri)
// and the browser build (WebUSB) implement Link; Device adds the typed
// commands on top, the same for both.

import {
  CMD,
  NbpError,
  START_FLUSH,
  core,
  type Decim,
  type Info,
  type Calibration,
  type Meta,
  type Points,
  type Reply,
} from './nbp';

export interface LinkEvents {
  onPoints(p: Points): void;
  onMeta(m: Meta): void;
  onClosed(reason: string): void;
}

/** Raw transport: one command at a time, its response. */
export interface Link {
  readonly description: string;
  request(cmd: number, payload?: Uint8Array, timeoutMs?: number): Promise<Reply>;
  /** samples per display point */
  setBucket(n: number): Promise<void>;
  /** codes to units, for samples from now on */
  setCalibration(c: Calibration): Promise<void>;
  close(): Promise<void>;
}

/** Something the user can pick in the connect list. */
export interface Candidate {
  key: string;
  label: string;
  detail: string;
  connect(ev: LinkEvents): Promise<Link>;
}

export class Device {
  private queue: Promise<unknown> = Promise.resolve();

  constructor(public link: Link) {}

  /** Commands go out one at a time; callers may overlap. */
  async cmd(cmd: number, payload: Uint8Array = new Uint8Array(), timeoutMs?: number): Promise<Uint8Array> {
    const run = async () => {
      const r = await this.link.request(cmd, payload, timeoutMs);
      if (r.status !== 0) throw new NbpError(cmd, r.status);
      return r.payload;
    };
    const p = this.queue.then(run, run);
    this.queue = p.catch(() => undefined);
    return p;
  }

  async info(): Promise<Info> {
    return core.parse_info(await this.cmd(CMD.GET_INFO));
  }

  async setSource(src: number) {
    await this.cmd(CMD.SET_SOURCE, core.enc_source(src));
  }

  async setDecimation(log2n: number, mode: number): Promise<Decim> {
    return core.parse_decim(await this.cmd(CMD.SET_DECIMATION, core.enc_decim(log2n, mode)));
  }

  async setRange(automatic: boolean, manual: number, min: number, max: number) {
    await this.cmd(CMD.SET_RANGE, core.enc_range_set(automatic, manual, min, max));
  }

  async start() {
    await this.cmd(CMD.STREAM_START, core.enc_stream_start(START_FLUSH));
  }

  async stop() {
    await this.cmd(CMD.STREAM_STOP);
  }

  async stats() {
    return core.parse_stats(await this.cmd(CMD.GET_STATS));
  }
}

export const isNative = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
