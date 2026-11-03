// Browser transport: WebUSB (Chromium browsers). The receive pipeline is the
// same Rust code as the native build, compiled to WASM.
//
// Linux needs the udev rule (udev/). Windows needs the device to bind WinUSB,
// which wants MS OS 2.0 descriptors in the firmware. Browsers have no raw
// TCP / UDP, so Ethernet is native-only for now.

import type { Candidate, Link, LinkEvents } from './device';
import {
  USB_CLASS,
  USB_PID_FS,
  USB_PID_HS,
  USB_PROTOCOL,
  USB_SUBCLASS,
  USB_VID,
  core,
  decodePoints,
  type Calibration,
  type Reply,
} from './nbp';

const FILTERS: USBDeviceFilter[] = [
  { vendorId: USB_VID, productId: USB_PID_HS },
  { vendorId: USB_VID, productId: USB_PID_FS },
];
const IN_TRANSFER = 16 * 1024;
const IN_QUEUE = 8;
const BATCH_MS = 40;

export const webUsbAvailable = typeof navigator !== 'undefined' && 'usb' in navigator;

class WebUsbLink implements Link {
  private pipeline = new core.Pipeline();
  private seq = 0;
  private pending: { seq: number; resolve: (r: Reply) => void; reject: (e: Error) => void } | null = null;
  private outEp = 0;
  private closed = false;
  private timer: ReturnType<typeof setInterval> | undefined;
  description: string;

  constructor(
    private dev: USBDevice,
    private ev: LinkEvents,
  ) {
    this.description = `WebUSB ${dev.productName ?? 'Nebulosa'}${dev.serialNumber ? ' ' + dev.serialNumber : ''}`;
  }

  async open() {
    const d = this.dev;
    await d.open();
    if (!d.configuration) await d.selectConfiguration(1);
    let found: { iface: USBInterface; alt: USBAlternateInterface } | undefined;
    for (const iface of d.configuration!.interfaces)
      for (const alt of iface.alternates)
        if (
          alt.interfaceClass === USB_CLASS &&
          alt.interfaceSubclass === USB_SUBCLASS &&
          alt.interfaceProtocol === USB_PROTOCOL
        )
          found ??= { iface, alt };
    if (!found) throw new Error('no NBP interface on this device');
    await d.claimInterface(found.iface.interfaceNumber);
    if (found.alt.alternateSetting !== 0)
      await d.selectAlternateInterface(found.iface.interfaceNumber, found.alt.alternateSetting);
    const bulk = found.alt.endpoints.filter((e) => e.type === 'bulk');
    const out = bulk.find((e) => e.direction === 'out');
    const ins = bulk.filter((e) => e.direction === 'in');
    if (!out || ins.length === 0) throw new Error('NBP interface without bulk endpoints');
    this.outEp = out.endpointNumber;
    ins.forEach((e, src) => void this.readLoop(e.endpointNumber, src));
    this.timer = setInterval(() => {
      const b = this.pipeline.take_batch();
      if (b) {
        this.ev.onPoints(decodePoints(b));
        this.ev.onMeta(this.pipeline.meta());
      }
    }, BATCH_MS);
    navigator.usb.addEventListener('disconnect', this.onDisconnect);
  }

  private onDisconnect = (e: USBConnectionEvent) => {
    if (e.device === this.dev) this.fail('device unplugged');
  };

  private async readLoop(ep: number, src: number) {
    const inflight: Promise<USBInTransferResult>[] = [];
    try {
      for (let i = 0; i < IN_QUEUE; i++) inflight.push(this.dev.transferIn(ep, IN_TRANSFER));
      while (!this.closed) {
        const r = await inflight.shift()!;
        if (!this.closed) inflight.push(this.dev.transferIn(ep, IN_TRANSFER));
        if (r.status !== 'ok') throw new Error(`USB IN ${ep}: ${r.status}`);
        if (r.data && r.data.byteLength) {
          this.pipeline.push(src, new Uint8Array(r.data.buffer, r.data.byteOffset, r.data.byteLength));
          this.deliver();
        }
      }
    } catch (e) {
      this.fail(e instanceof Error ? e.message : String(e));
    }
  }

  private deliver() {
    const rs = this.pipeline.take_responses() as { ty: number; status: number; seq: number; payload: number[] }[];
    for (const r of rs) {
      if (this.pending && this.pending.seq === r.seq) {
        const p = this.pending;
        this.pending = null;
        p.resolve({ ty: r.ty, status: r.status, payload: new Uint8Array(r.payload) });
      } else console.warn('unexpected response', r);
    }
  }

  private fail(reason: string) {
    if (this.closed) return;
    this.teardown();
    this.ev.onClosed(reason);
  }

  private teardown() {
    this.closed = true;
    clearInterval(this.timer);
    navigator.usb.removeEventListener('disconnect', this.onDisconnect);
    this.pending?.reject(new Error('connection closed'));
    this.pending = null;
  }

  async request(cmd: number, payload = new Uint8Array(), timeoutMs = 1500): Promise<Reply> {
    if (this.closed) throw new Error('not connected');
    const seq = (this.seq = (this.seq + 1) & 0xffff);
    const reply = new Promise<Reply>((resolve, reject) => {
      this.pending = { seq, resolve, reject };
    });
    let to: ReturnType<typeof setTimeout> | undefined;
    const timeout = new Promise<never>((_, reject) => {
      to = setTimeout(() => {
        if (this.pending?.seq === seq) this.pending = null;
        reject(new Error(`no response to command 0x${cmd.toString(16)}`));
      }, timeoutMs);
    });
    try {
      await this.dev.transferOut(this.outEp, core.encode(cmd, seq, payload) as Uint8Array<ArrayBuffer>);
      return await Promise.race([reply, timeout]);
    } finally {
      clearTimeout(to);
    }
  }

  async setBucket(n: number) {
    this.pipeline.set_bucket(n);
  }

  async setCalibration(c: Calibration) {
    this.pipeline.set_calibration(c);
  }

  async close() {
    if (this.closed) return;
    this.teardown();
    try {
      await this.dev.close();
    } catch {
      /* already gone */
    }
  }
}

function candidate(dev: USBDevice): Candidate {
  return {
    key: `webusb:${dev.serialNumber ?? ''}:${dev.productId}`,
    label: `${dev.manufacturerName ?? 'INSTRIX'} ${dev.productName ?? 'Nebulosa'}`,
    detail: `WebUSB · ${dev.productId === USB_PID_FS ? 'USB-C, Full-Speed' : 'USB 3 port'}${dev.serialNumber ? ' · ' + dev.serialNumber : ''}`,
    async connect(ev) {
      const l = new WebUsbLink(dev, ev);
      await l.open();
      return l;
    },
  };
}

/** Devices this site was already granted. */
export async function scan(): Promise<Candidate[]> {
  if (!webUsbAvailable) return [];
  return (await navigator.usb.getDevices()).filter((d) => FILTERS.some((f) => f.productId === d.productId)).map(candidate);
}

/** The browser's chooser; needs a user gesture. */
export async function pick(): Promise<Candidate | null> {
  try {
    return candidate(await navigator.usb.requestDevice({ filters: FILTERS }));
  } catch {
    return null; // cancelled
  }
}
