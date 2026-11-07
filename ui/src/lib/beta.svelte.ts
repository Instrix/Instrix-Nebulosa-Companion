// Beta features: hidden until turned on in Settings → Beta features. Each is
// kept per computer (localStorage); ?beta=sca (a comma list) turns one on for
// that page load, for links and screenshots.

export type BetaId = 'sca';

export interface BetaInfo {
  id: BetaId;
  label: string;
  detail: string;
}

export const BETAS: BetaInfo[] = [
  {
    id: 'sca',
    label: 'Side-channel analysis',
    detail: 'The Side-channel tab: capture power traces, t-test (TVLA), CPA key recovery, timing, export for ChipWhisperer. For slow targets; its tutorial and showcase too.',
  },
];

const key = (id: BetaId) => `nebulosa.beta.${id}`;

function load(id: BetaId): boolean {
  try {
    if (new URLSearchParams(location.search).get('beta')?.split(',').includes(id)) return true;
    return localStorage.getItem(key(id)) === '1';
  } catch {
    return false;
  }
}

/** which beta features are on (reactive) */
export const beta = $state<Record<BetaId, boolean>>(Object.fromEntries(BETAS.map((b) => [b.id, load(b.id)])) as Record<BetaId, boolean>);

export function setBeta(id: BetaId, on: boolean) {
  beta[id] = on;
  try {
    localStorage.setItem(key(id), on ? '1' : '0');
  } catch {
    /* storage unavailable */
  }
}
