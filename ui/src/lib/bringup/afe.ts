// The AFE1 ×10 shunt ladder and its three chains (Instrix-Nebulosa TR-0002
// "AFE1 ×10 analog front end", Rev A), for the bring-up: nominal values to
// turn an ADC code into a current, and the ranges' limits for the
// interlocks. Calibration replaces the nominal gains; these are what an
// uncalibrated board should read within ~1 %. (The Nebulosa Debugger's
// src/core/afe.h.)

export const TAPS = 6; // range r = tap r + 1
export const VREF = 2.5; // ADC2 reference
export const CODE_FS = 16384; // RS1473, 14 bits

/** cumulative ladder resistance from I+ to tap node t(r+1), Ω */
export const R_CUM = [0.01, 0.1, 1.0, 10.0, 100.0, 1000.0];
/** full scale, A */
export const FS_A = [2.0, 0.2, 20e-3, 2e-3, 200e-6, 20e-6];
export const FS_LABEL = ['2 A', '200 mA', '20 mA', '2 mA', '200 µA', '20 µA'];

/** chains: A on t3 (OPA2210, ×4.98 then ×5.00), B on t5 (same), C on t6 (OPA2387 pair, ×4.09 then ×6.12) */
export const CHAIN_GAIN = [112.4, 112.4, 112.75];
export const CHAIN_NAME = ['A (t3)', 'B (t5)', 'C (t6)'];

/**
 * Which chain's ADC the stream carries in range r. The gateware (FPGA ID
 * 0x4E42000B) still has the ×25 board's map (0/1 A, 2/3 B, 4/5 C); TR-0002
 * wants 0-2 A, 3-4 B, 5 C. Either way the reading is right at DC: with tap
 * k on, no current flows below t(k), so every chain on a node at or below
 * the tap reads the tap's voltage (Kelvin).
 */
export type ChainMap = 'x25' | 'x10';
export function chainOf(r: number, m: ChainMap): number {
  if (m === 'x25') return Math.floor(r / 2);
  return r <= 2 ? 0 : r <= 4 ? 1 : 2;
}

export const codeToVadc = (code: number) => (code * VREF) / CODE_FS;
/** nominal current from an (offset-corrected) ADC code */
export const codeToAmps = (code: number, r: number, m: ChainMap) => codeToVadc(code) / CHAIN_GAIN[chainOf(r, m)] / R_CUM[r];
export const ampsToCode = (a: number, r: number, m: ChainMap) => ((a * R_CUM[r] * CHAIN_GAIN[chainOf(r, m)]) / VREF) * CODE_FS;

/** the range comparators (AFE2): up at 90 % FS, down at 4.5 % FS (TR-0002 §Autoranging) */
export const COMP_UP = 0.9;
export const COMP_DOWN = 0.045;

/** shunt + switch burden (~20-25 mV at full scale): the loop voltage left for the load is the supply minus this */
export const burdenV = (amps: number, r: number) => amps * R_CUM[r] * 1.25;

/**
 * The loop current may not exceed 1.1 × FS of the tap that is on (taps 2-6:
 * the ladder below the tap sees body-diode current otherwise).
 */
export const ilLimit = (r: number) => (r < 0 || r >= TAPS ? 0 : 1.1 * FS_A[r]);

/** "1.2345 mA" */
export function siAmps(a: number | null | undefined): string {
  if (a == null || Number.isNaN(a)) return '—';
  const m = Math.abs(a);
  if (m >= 0.1) return `${a.toFixed(4)} A`;
  if (m >= 1e-4) return `${(a * 1e3).toFixed(4)} mA`;
  if (m >= 1e-7) return `${(a * 1e6).toFixed(4)} µA`;
  return `${(a * 1e9).toFixed(2)} nA`;
}

export function ohms(r: number): string {
  if (r >= 1e6) return `${+(r / 1e6).toPrecision(4)} MΩ`;
  if (r >= 1e3) return `${+(r / 1e3).toPrecision(4)} kΩ`;
  return `${+r.toPrecision(4)} Ω`;
}

/** a decade box setting a person can dial: 4 significant digits */
export function boxRound(r: number): number {
  if (r <= 0) return r;
  const e = 10 ** (Math.floor(Math.log10(r)) - 3);
  return Math.round(r / e) * e;
}
