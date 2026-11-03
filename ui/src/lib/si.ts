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

/** `digits` significant digits, with the prefix chosen from the value itself. */
export function fmtSI(v: number, unit: string, digits = 4): string {
  if (!isFinite(v)) return '–';
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
