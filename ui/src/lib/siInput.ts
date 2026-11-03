// `use:siInput={{ get, set, unit, v }}`: a text field for a value in SI units
// (shows 10.00 mA, takes 10m / 10 mA / 0.01).

import { fmtSI, parseSI } from './si';

/** an SI text field: shows `value`, writes back what parses. Pass `v` (the value) so outside changes show. */
export function siInput(node: HTMLInputElement, p: { get: () => number; set: (v: number) => void; unit: string; v?: number }) {
  let cur = p;
  const show = () => (node.value = fmtSI(cur.get(), cur.unit, 4));
  const commit = () => {
    const v = parseSI(node.value);
    if (isFinite(v)) cur.set(v);
    show();
  };
  show();
  node.addEventListener('change', commit);
  return {
    update(n: typeof p) {
      cur = n;
      if (document.activeElement !== node) show();
    },
    destroy() {
      node.removeEventListener('change', commit);
    },
  };
}
