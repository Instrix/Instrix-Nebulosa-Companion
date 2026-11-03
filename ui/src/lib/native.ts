// Native transport: the Rust backend (src-tauri) owns USB / TCP / UDP and
// runs the receive pipeline; batches arrive on a Tauri channel.

import { Channel, invoke } from '@tauri-apps/api/core';
import type { Candidate, Link, LinkEvents } from './device';
import { decodePoints, hex, type Calibration, type Info, type Meta, type Reply } from './nbp';

type Event = ArrayBuffer | ({ event: 'meta' } & Meta) | { event: 'closed'; reason: string };

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
  constructor(public description: string) {}

  async request(cmd: number, payload = new Uint8Array(), timeoutMs?: number): Promise<Reply> {
    const r = await invoke<{ ty: number; status: number; payload: number[] }>('request', {
      cmd,
      payload: Array.from(payload),
      timeoutMs,
    });
    return { ty: r.ty, status: r.status, payload: new Uint8Array(r.payload) };
  }

  setBucket(n: number) {
    return invoke<void>('set_bucket', { n });
  }

  setCalibration(cal: Calibration) {
    return invoke<void>('set_calibration', { cal });
  }

  close() {
    return invoke<void>('disconnect');
  }
}

async function open(target: Target, ev: LinkEvents): Promise<Link> {
  const events = new Channel<Event>();
  events.onmessage = (m) => {
    if (m instanceof ArrayBuffer) ev.onPoints(decodePoints(new Float64Array(m)));
    else if (m.event === 'meta') ev.onMeta(m);
    else ev.onClosed(m.reason);
  };
  const description = await invoke<string>('connect', { target, events });
  return new NativeLink(description);
}

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
