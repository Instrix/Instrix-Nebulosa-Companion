// Native transport: the Rust backend (src-tauri) owns USB / TCP / UDP and
// runs the receive pipeline; batches arrive on a Tauri channel.

import { Channel, invoke } from '@tauri-apps/api/core';
import type { Candidate, Link, LinkEvents } from './device';
import { hex, type Calibration, type Info, type Meta, type RawSpanStats, type Reply, type TrigEvent, type TriggerCfg } from './nbp';

type Event = ({ event: 'meta' } & Meta) | { event: 'closed'; reason: string };

interface UsbDev {
  id: string;
  label: string;
  serial: string | null;
  speed: string | null;
}

interface NetDev {
  host: string;
  info: Info;
}

type Target = { kind: 'usb'; id: string } | { kind: 'tcp'; host: string; port?: number };

class NativeLink implements Link {
  constructor(
    public description: string,
    public playback = false,
  ) {}

  async request(cmd: number, payload = new Uint8Array(), timeoutMs?: number): Promise<Reply> {
    if (this.playback) throw new Error('a recording: no device to command');
    const r = await invoke<{ ty: number; status: number; payload: number[] }>('request', {
      cmd,
      payload: Array.from(payload),
      timeoutMs,
    });
    return { ty: r.ty, status: r.status, payload: new Uint8Array(r.payload) };
  }

  setBuffer(samples: number) {
    return invoke<void>('set_buffer', { samples });
  }

  async lod(t0: number, t1: number, bins: number) {
    return new Float64Array(await invoke<ArrayBuffer>('query_lod', { t0, t1, bins }));
  }

  stats(t0: number, t1: number) {
    return invoke<RawSpanStats | null>('query_stats', { t0, t1 });
  }

  valueAt(t: number) {
    return invoke<[number, number, number] | null>('query_value', { t });
  }

  setCalibration(cal: Calibration) {
    return invoke<void>('set_calibration', { cal });
  }

  setTrigger(cfg: TriggerCfg) {
    return invoke<void>('set_trigger', { cfg });
  }

  events(since: number, max: number) {
    return invoke<TrigEvent[]>('query_events', { since, max });
  }

  clearEvents() {
    return invoke<void>('clear_events');
  }

  close() {
    return invoke<void>('disconnect');
  }
}

function channel(ev: LinkEvents) {
  const events = new Channel<Event>();
  events.onmessage = (m) => {
    if (m.event === 'meta') ev.onMeta(m);
    else ev.onClosed(m.reason);
  };
  return events;
}

async function open(target: Target, ev: LinkEvents): Promise<Link> {
  const description = await invoke<string>('connect', { target, events: channel(ev) });
  return new NativeLink(description);
}

// ---- files -------------------------------------------------------------------

export interface RecFile {
  path: string;
  name: string;
  bytes: number;
  modified: number;
}

export const listRecordings = () => invoke<RecFile[]>('list_recordings');
export const recordingsDir = () => invoke<string>('recordings_dir');
export const startRecording = (header: unknown) => invoke<string>('start_recording', { header: JSON.stringify(header) });
export const stopRecording = () =>
  invoke<{ path: string; samples: number; bytes: number; seconds: number } | null>('stop_recording');
export const countSamples = (t0: number, t1: number) => invoke<number>('count_samples', { t0, t1 });

/** Opens a recording for playback: a Link to it, and the header it was written with. */
export async function openRecording(path: string, ev: LinkEvents): Promise<{ link: Link; header: Record<string, unknown> }> {
  const header = await invoke<string>('open_recording', { path, events: channel(ev) });
  const name = path.split(/[\\/]/).pop() ?? path;
  let h: Record<string, unknown> = {};
  try {
    h = JSON.parse(header);
  } catch {
    /* an unreadable header: play it anyway */
  }
  return { link: new NativeLink(`Recording ${name}`, true), header: h };
}

export async function pickRecording(): Promise<string | null> {
  const { open: dialog } = await import('@tauri-apps/plugin-dialog');
  const p = await dialog({ multiple: false, directory: false, filters: [{ name: 'Nebulosa recording', extensions: ['nbrec'] }] });
  return typeof p === 'string' ? p : null;
}

export async function pickSave(defaultPath: string, name: string, ext: string): Promise<string | null> {
  const { save } = await import('@tauri-apps/plugin-dialog');
  return save({ defaultPath, filters: [{ name, extensions: [ext] }] });
}

export const exportCsv = (t0: number, t1: number, path: string) => invoke<number>('export_csv', { t0, t1, path });
export const saveBytes = (path: string, bytes: Uint8Array) => invoke<void>('save_bytes', { path, bytes: Array.from(bytes) });

export function tcpCandidate(host: string, port?: number): Candidate {
  return {
    key: `tcp:${host}:${port ?? ''}`,
    label: host,
    detail: 'Ethernet',
    connect: (ev) => open({ kind: 'tcp', host, port }, ev),
  };
}

export async function scan(): Promise<Candidate[]> {
  const [usb, net] = await Promise.all([
    invoke<UsbDev[]>('list_usb').catch((e) => {
      console.warn('USB list', e);
      return [] as UsbDev[];
    }),
    invoke<NetDev[]>('discover', { timeoutMs: 600 }).catch((e) => {
      console.warn('discovery', e);
      return [] as NetDev[];
    }),
  ]);
  return [
    ...usb.map(
      (d): Candidate => ({
        key: `usb:${d.id}`,
        label: d.label,
        detail: ['USB', d.speed, d.serial].filter(Boolean).join(' · '),
        connect: (ev) => open({ kind: 'usb', id: d.id }, ev),
      }),
    ),
    ...net.map(
      (d): Candidate => ({
        ...tcpCandidate(d.host),
        label: `Nebulosa ${d.info.fw_version_str}`,
        detail: `Ethernet · ${d.host} · ${d.info.mac || 'no MAC'} · FPGA ${hex(d.info.fpga_id)}`,
      }),
    ),
  ];
}
