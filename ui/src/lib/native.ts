// Native transport: the Rust backend (src-tauri) owns USB / TCP / UDP and
// runs the receive pipeline; batches arrive on a Tauri channel.

import { Channel, invoke } from '@tauri-apps/api/core';
import type { Candidate, Link, LinkEvents } from './device';
import { hex, type ExportCfg, type AnalysisRequest, type AnalysisResult, type Calibration, type Info, type Meta, type RawSpanStats, type Reply, type TrigEvent, type TriggerCfg } from './nbp';

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
    return invoke<[number, number, number, number] | null>('query_value', { t });
  }

  async sessionLod(t0: number, t1: number, bins: number) {
    return new Float64Array(await invoke<ArrayBuffer>('session_lod', { t0, t1, bins }));
  }

  sessionStats(t0: number, t1: number) {
    return invoke<RawSpanStats | null>('session_stats', { t0, t1 });
  }

  setLogInterval(interval: number) {
    return invoke<void>('set_log_interval', { interval });
  }

  clearSession() {
    return invoke<void>('clear_session');
  }

  exportPreview(cfg: ExportCfg, wallOffset: number, rows: number) {
    return invoke<string>('export_preview', { cfg, wallOffset, rows });
  }

  async setMath(expr: string | null) {
    try {
      await invoke<void>('set_math', { expr });
      return null;
    } catch (e) {
      return String(e);
    }
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

  analyze(t0: number, t1: number, req: AnalysisRequest) {
    return invoke<AnalysisResult>('analyze', { t0, t1, req });
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
  const p = await dialog({
    multiple: false,
    directory: false,
    filters: [{ name: 'Nebulosa recording or log', extensions: ['nbrec', 'nblog'] }],
  });
  return typeof p === 'string' ? p : null;
}

export async function pickSave(defaultPath: string, name: string, ext: string): Promise<string | null> {
  const { save } = await import('@tauri-apps/plugin-dialog');
  return save({ defaultPath, filters: [{ name, extensions: [ext] }] });
}

 // ---- logging ----------------------------------------------------------------

export const startLogging = (header: unknown) => invoke<string>('start_logging', { header: JSON.stringify(header) });
export const stopLogging = () => invoke<{ path: string; entries: number; bytes: number } | null>('stop_logging');
export const diskStatus = () =>
  invoke<{
    free: number;
    recording_error: string | null;
    logging_error: string | null;
    export_error: string | null;
    export_bytes: number;
    dropped: number;
  }>('disk_status');

/** Opens a session log (.nblog) for viewing. */
export async function openLog(path: string, ev: LinkEvents): Promise<{ link: Link; header: Record<string, unknown> }> {
  const header = await invoke<string>('open_log', { path, events: channel(ev) });
  const name = path.split(/[\\/]/).pop() ?? path;
  let h: Record<string, unknown> = {};
  try {
    h = JSON.parse(header);
  } catch {
    /* an unreadable header: show it anyway */
  }
  return { link: new NativeLink(`Log ${name}`, true), header: h };
}

export const startExport = (cfg: ExportCfg, wallOffset: number, header: unknown) =>
  invoke<string>('start_export', { cfg, wallOffset, header: JSON.stringify(header) });
export const stopExport = () => invoke<{ path: string; rows: number; bytes: number } | null>('stop_export');

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
