// SI formatting and parsing: 0.0123 A -> "12.30 mA"; "10m" -> 0.01.

const PREFIXES: [number, string][] = [
  [1e12, 'T'],
  [1e9, 'G'],
  [1e6, 'M'],
  [1e3, 'k'],
  [1, ''],
  [1e-3, 'm'],
  [1e-6, 'µ'],
  [1e-9, 'n'],
  [1e-12, 'p'],
  [1e-15, 'f'],
];

/** The prefix for magnitude `a`. */
export function prefixFor(a: number): [number, string] {
  a = Math.abs(a);
  if (!isFinite(a) || a === 0) return [1, ''];
  for (const p of PREFIXES) if (a >= p[0] * 0.9995) return p;
  return PREFIXES[PREFIXES.length - 1];
}

/** a unit that carries its own prefix (mA, mW, µA, kΩ): no other prefix goes on it */
const PREFIXED = /^[pnuµmkMG][A-Za-zΩ°]/;

/** `digits` significant digits, with the prefix chosen from the value itself. */
export function fmtSI(v: number, unit: string, digits = 4): string {
  if (!isFinite(v)) return '–';
  if (PREFIXED.test(unit)) {
    const a = Math.abs(v);
    const int = a >= 100 ? 3 : a >= 10 ? 2 : 1;
    const txt = a === 0 ? '0' : a >= 1e4 ? v.toFixed(0) : a < 0.01 ? v.toPrecision(3) : v.toFixed(Math.max(0, digits - int));
    return `${txt} ${unit}`;
  }
  const [f, p] = prefixFor(v);
  const x = v / f;
  const int = Math.abs(x) >= 100 ? 3 : Math.abs(x) >= 10 ? 2 : 1;
  return `${x.toFixed(Math.max(0, digits - int))} ${p}${unit}`;
}

/** Formats with a given prefix (for aligned columns / axes). */
export function fmtWith(v: number, prefix: [number, string], unit: string, decimals: number): string {
  if (!isFinite(v)) return '–';
  return `${(v / prefix[0]).toFixed(decimals)} ${prefix[1]}${unit}`;
}

const PARSE: Record<string, number> = { T: 1e12, G: 1e9, M: 1e6, k: 1e3, m: 1e-3, u: 1e-6, µ: 1e-6, n: 1e-9, p: 1e-12, f: 1e-15 };

/** "10m", "2.5 µA", "-3e-3", "200uA" -> number; NaN if it isn't one. */
export function parseSI(s: string): number {
  const m = s.trim().match(/^([-+]?\d*\.?\d+(?:e[-+]?\d+)?)\s*([TGMkmuµnpf]?)\s*[A-Za-z]*$/);
  if (!m) return NaN;
  return parseFloat(m[1]) * (m[2] ? PARSE[m[2]] : 1);
}

/** Evenly spaced ticks: n divisions of [min, max]. */
export function evenSplits(n: number) {
  return (_u: unknown, _i: number, min: number, max: number) => Array.from({ length: n + 1 }, (_, k) => min + (k * (max - min)) / n);
}

/**
 * Log axis ticks: every decade, plus 2 and 5 within it when there are few
 * decades; evenly spaced within a single decade. (uPlot's own log splitter
 * can loop forever when its increment comes out as a non-power of ten.)
 */
export function logSplits(_u: unknown, _i: number, min: number, max: number): number[] {
  if (!(min > 0) || !(max > min)) return [];
  const lo = Math.floor(Math.log10(min));
  const hi = Math.ceil(Math.log10(max));
  const mults = hi - lo <= 3 ? [1, 2, 5] : [1];
  const out: number[] = [];
  for (let e = lo; e <= hi && out.length < 64; e++)
    for (const k of mults) {
      const v = k * 10 ** e;
      if (v >= min && v <= max) out.push(v);
    }
  return out.length >= 2 ? out : evenSplits(4)(_u, _i, min, max);
}
