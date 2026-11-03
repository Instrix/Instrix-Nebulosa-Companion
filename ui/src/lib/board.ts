// Board commands (NBP_CAP_BOARD): the status (inputs, USB PD, analog
// supplies, clock, SD card, FPGA), settings and self-tests. Layouts are the
// Rust core's (board.rs).

import type { Device } from './device';
import { NbpError, core } from './nbp';

export const CMD_BOARD_STATUS = 0x38;
export const CMD_BOARD_SET = 0x39;
export const CMD_BOARD_TEST = 0x3a;
export const CAP_BOARD = 1 << 4;

export const SET = { LEDS: 0, APWR_H: 1, APWR_L: 2, USB3_EN: 3, USB3_DIR: 4, PWR_SEL: 5, FPGA_PROGRAM: 6, LCD: 7, RTC: 8, PD_MV: 9, UI_KEY: 10 } as const;
export const TEST_ALL = (1 << 12) - 1;

export interface TestResult {
  name: string;
  what: string;
  result: 'none' | 'pass' | 'fail' | 'skip';
  detail: number;
  detail_unit: string;
}

export interface Pdo {
  mv: number;
  ma: number;
  fixed: boolean;
}

export interface BoardStatus {
  board_id: number;
  uptime_ms: number;
  core_hz: number;
  hclk_hz: number;
  flags: number;
  rtc_s: number;
  v_bj_mv: number;
  v_vbus_mv: number;
  v_main_mv: number;
  leds: number;
  led_manual: boolean;
  pwr_sel: number;
  cc: number;
  fpga_mode: number;
  lcd_mode: number;
  sd_type: number;
  fpga_cfg_ms: number;
  fpga_cfg_bytes: number;
  fpga_id: number;
  flash_jedec: number;
  flash_bytes: number;
  sd_blocks: number;
  tests: TestResult[];
  pd_state: number;
  pd_cc_line: number;
  pd_spec_rev: number;
  pd_mv: number;
  pd_ma: number;
  pd_target_mv: number;
  pd_pdo_sel: number;
  pd_pdos: Pdo[];
  pd_contracts: number;
  pd_hard_resets: number;
  pd_errors: number;
  apwr_h: boolean;
  apwr_l: boolean;
  apwr_ok: boolean;
  usb_c: boolean;
  usb_hs: boolean;
  usb_ss: boolean;
  eth_link: boolean;
  sd_card: boolean;
  rtc_running: boolean;
  fpga_done: boolean;
}

export const PD_STATE = ['no USB-C source', 'waiting for the source', 'negotiating', 'negotiating', 'contract', 'no PD (5 V)'];
export const PWR_SEL = ['Auto', '12 V jack', 'USB-C'];

/** A board command; the status comes back with the status payload whatever happened. */
async function call(d: Device, cmd: number, payload: Uint8Array, timeoutMs = 2000): Promise<{ status: number; board: BoardStatus | null }> {
  const r = await d.link.request(cmd, payload, timeoutMs);
  return { status: r.status, board: core.parse_board_status(r.payload) ?? null };
}

/** null when the firmware has no board commands */
export async function status(d: Device): Promise<BoardStatus | null> {
  const r = await call(d, CMD_BOARD_STATUS, new Uint8Array());
  if (r.status === 1) return null; // NBP_ST_UNKNOWN
  if (r.status !== 0) throw new NbpError(CMD_BOARD_STATUS, r.status);
  return r.board;
}

/** a setting; throws with the board's reason (NO_POWER, BUSY, ...) and still hands back the status */
export async function set(d: Device, item: number, value: number): Promise<BoardStatus | null> {
  const r = await call(d, CMD_BOARD_SET, core.enc_board_set(item, value >>> 0));
  if (r.status !== 0) throw Object.assign(new NbpError(CMD_BOARD_SET, r.status), { board: r.board });
  return r.board;
}

/** runs the self-tests (not while streaming); the results are in the status's tests */
export async function selfTest(d: Device, mask = TEST_ALL): Promise<{ ok: boolean; board: BoardStatus | null }> {
  const r = await call(d, CMD_BOARD_TEST, core.enc_board_test(mask), 15000);
  if (r.status !== 0 && r.status !== 6) throw new NbpError(CMD_BOARD_TEST, r.status); // 6: HW, a test failed
  return { ok: r.status === 0, board: r.board };
}

/** the board's clock to this computer's (UTC seconds) */
export function syncClock(d: Device) {
  return set(d, SET.RTC, Math.round(Date.now() / 1000));
}

/** human words for the board's refusals */
export function reason(e: unknown): string {
  if (e instanceof NbpError) {
    if (e.status === 7) return 'needs a 12 V source: the barrel jack, or a USB-C charger that offers 12 V';
    if (e.status === 5) return 'not while streaming: press Stop first';
    if (e.status === 3) return 'the board refused that value';
  }
  return String(e instanceof Error ? e.message : e);
}
