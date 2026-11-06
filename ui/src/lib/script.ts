// The script trigger's Python, run by the desktop backend (src-tauri script.rs):
// loading the script, scanning a block of samples with it. Not in the
// browser: it can't start programs.

import { invoke } from './ipc';

export interface ScriptInfo {
  python: string;
  numpy: string | null;
}

/** a trigger the script found */
export interface Found {
  t: number;
  label: string | null;
  width: number | null;
  peak: number | null;
  mean: number | null;
  integral: number | null;
}

export interface ScanResult {
  /** where the next scan starts, s */
  next: number;
  /** samples scanned, triggers found */
  n: number;
  found: number;
  /** the triggers, when the block was the UI's (the backend's own go straight into its log) */
  events: Found[];
  out: string;
  error: string | null;
  ms: number;
}

/** starts the script (python: the interpreter, '' for python3 / python): the versions, or why not */
export function scriptLoad(python: string, code: string) {
  return invoke<ScriptInfo>('script_load', { python, code });
}

/** The script on the samples in [t0, t1): `block` (base64, from the UI's pipeline), or (null) the backend's own. */
export function scriptScan(python: string, code: string, t0: number, t1: number, max: number, block: string | null, timeoutMs = 5000) {
  return invoke<ScanResult>('script_scan', { req: { python, code, t0, t1, max, block, timeoutMs } });
}

export interface CheckResult {
  /** it loaded: Python's version */
  info: ScriptInfo | null;
  /** why not, or the dry run's traceback */
  error: string | null;
  /** samples of the dry run (0: none), and the time they cover, s */
  n: number;
  span: number;
  found: number;
  /** the first triggers it found */
  events: Found[];
  out: string;
  ms: number;
}

/** Checks a script: it loads, and a dry run on [t0, t1) (`block`, or the backend's own samples). */
export function scriptCheck(python: string, code: string, t0: number, t1: number, block: string | null) {
  return invoke<CheckResult>('script_check', { req: { python, code, t0, t1, max: 1_000_000, block, timeoutMs: 10_000 } });
}

/** the line of the script a traceback or syntax error points at */
export function errorLine(err: string): number | null {
  const all = [...err.matchAll(/"<trigger script>", line (\d+)/g)];
  return all.length ? +all[all.length - 1][1] : null;
}

/** the trigger starts over: the script's reset(), if it has one */
export function scriptReset(python: string, code: string) {
  return invoke<void>('script_reset', { python, code });
}

export function scriptStop() {
  return invoke<void>('script_stop');
}
