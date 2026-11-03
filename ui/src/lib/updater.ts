// Software update over NBP, any link (USB, Ethernet, WebUSB): the steps of
// the Instrix-Nebulosa-Debugger's `update` (board.cpp), on Device commands.
//
//   1. UPDATE INFO: where the update slot is, how big
//   2. erase it (64 KB at a time), write the image (256-byte pages, blank
//      ones skipped), read it back and compare
//   3. UPDATE GATEWARE (the board test-loads it and only then makes it
//      active, else the old one comes back) or UPDATE FIRMWARE (checked,
//      staged; the board restarts and its bootloader installs it)
//
// Layouts and checks (CRC-32, .nbfw, .bit) are the Rust core's (WASM).

import type { Device } from './device';
import { CMD, NbpError, core } from './nbp';

export const CMD_FLASH_READ = 0x3b;
export const CMD_FLASH_WRITE = 0x3c;
export const CMD_FLASH_ERASE = 0x3d;
export const CMD_UPDATE = 0x3f;
const UPD = { INFO: 0, GATEWARE: 1, FIRMWARE: 2, REVERT: 3, RECOVERY: 4 } as const;
const PAGE = 256;
const ERASE = 65536;
const ST_UNKNOWN = 1;

export interface Slot {
  kind: number; // 0 empty, 1 gateware, 2 firmware
  len: number;
  crc: number;
  id: number;
}

export interface UpdateInfo {
  fw_version: number;
  fw_version_str: string;
  fw_build_id: number;
  fw_len: number;
  fw_crc: number;
  fw_date: string;
  boot_version: number;
  active: number;
  update_slot: number;
  fw_pending: boolean;
  result: number;
  slot_size: number;
  slots: [Slot, Slot];
  installs: number;
  msg: string;
}

export type Progress = (stage: string, done: number, total: number) => void;

export class Cancelled extends Error {
  constructor() {
    super('cancelled');
  }
}

/** UPDATE with an op; the info that comes back even when the status isn't OK */
async function op(d: Device, o: number, slot = 0, len = 0, crc = 0, timeoutMs = 2000): Promise<{ status: number; info: UpdateInfo | null }> {
  const r = await d.link.request(CMD_UPDATE, core.enc_update(o, slot, len, crc), timeoutMs);
  return { status: r.status, info: core.parse_update_info(r.payload) };
}

/** What runs and what's in the slots; null when the firmware has no software update. */
export async function info(d: Device): Promise<UpdateInfo | null> {
  const r = await op(d, UPD.INFO);
  if (r.status === ST_UNKNOWN) return null;
  if (r.status !== 0) throw new NbpError(CMD_UPDATE, r.status);
  return r.info;
}

/** Erase, write, verify `data` at `addr` (4 KB aligned). */
async function upload(d: Device, addr: number, data: Uint8Array, progress: Progress, cancel: () => boolean) {
  const end = addr + Math.ceil(data.length / 4096) * 4096;
  for (let a = addr; a < end; a += ERASE) {
    if (cancel()) throw new Cancelled();
    await d.cmd(CMD_FLASH_ERASE, core.enc_flash(a, Math.min(ERASE, end - a), new Uint8Array()), 5000);
    progress('Erasing', a + Math.min(ERASE, end - a) - addr, end - addr);
  }
  for (let off = 0; off < data.length; off += PAGE) {
    if (cancel()) throw new Cancelled();
    const chunk = data.subarray(off, Math.min(off + PAGE, data.length));
    if (!chunk.every((b) => b === 0xff)) await d.cmd(CMD_FLASH_WRITE, core.enc_flash(addr + off, chunk.length, chunk));
    if ((off / PAGE) % 16 === 0) progress('Writing', off + chunk.length, data.length);
  }
  progress('Writing', data.length, data.length);
  for (let off = 0; off < data.length; off += PAGE) {
    if (cancel()) throw new Cancelled();
    const n = Math.min(PAGE, data.length - off);
    const r = await d.cmd(CMD_FLASH_READ, core.enc_flash(addr + off, n, new Uint8Array()));
    const got = r.subarray(8, 8 + n);
    for (let k = 0; k < n; k++)
      if (got[k] !== data[off + k]) throw new Error(`verify: byte ${addr + off + k} reads 0x${got[k]?.toString(16)} not 0x${data[off + k].toString(16)}`);
    if ((off / PAGE) % 16 === 0) progress('Verifying', off + n, data.length);
  }
  progress('Verifying', data.length, data.length);
}

export interface Outcome {
  ok: boolean;
  status: number;
  info: UpdateInfo | null;
  /** what the board said */
  msg: string;
}

async function install(
  d: Device,
  kind: 'gateware' | 'firmware',
  image: Uint8Array,
  progress: Progress,
  cancel: () => boolean,
): Promise<Outcome> {
  const i = await info(d);
  if (!i) throw new Error('this firmware has no software update');
  if (image.length === 0 || image.length > i.slot_size)
    throw new Error(`the image is ${image.length} bytes; the slot holds ${i.slot_size}`);
  await upload(d, i.update_slot * i.slot_size, image, progress, cancel);
  progress(kind === 'gateware' ? 'Testing the gateware on the FPGA' : 'Checking the image', 0, 1);
  const r = await op(d, kind === 'gateware' ? UPD.GATEWARE : UPD.FIRMWARE, i.update_slot, image.length, core.crc32(image), kind === 'gateware' ? 15000 : 10000);
  progress('Done', 1, 1);
  return { ok: r.status === 0, status: r.status, info: r.info, msg: r.info?.msg ?? core.status_name(r.status) };
}

/** A gateware file (.bit / .bin): tested on the FPGA, then made active (or the old one kept). */
export function installGateware(d: Device, file: Uint8Array, progress: Progress, cancel: () => boolean) {
  return install(d, 'gateware', core.bitstream(file), progress, cancel);
}

/** A firmware image (.nbfw): checked, staged; the board restarts to install it. */
export function installFirmware(d: Device, img: Uint8Array, progress: Progress, cancel: () => boolean) {
  const c = core.firmware_check(img);
  if (!c.ok) throw new Error(c.error);
  return install(d, 'firmware', img, progress, cancel);
}

export async function revert(d: Device): Promise<Outcome> {
  const r = await op(d, UPD.REVERT, 0, 0, 0, 15000);
  return { ok: r.status === 0, status: r.status, info: r.info, msg: r.info?.msg ?? core.status_name(r.status) };
}

/** Restart into the H417's ROM USB bootloader (WCH ISP): the connection goes. */
export async function recovery(d: Device) {
  await op(d, UPD.RECOVERY).catch(() => undefined);
}

export { CMD };
