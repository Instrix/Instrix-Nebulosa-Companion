// UI scale for small and large screens. The desktop app zooms its webview
// (like a browser's zoom: crisp, and the plots' pointer maths stays right);
// in a browser the browser's own zoom does it (Ctrl + / − / 0).

import { isNative } from './device';

export const SCALES = [0.75, 0.8, 0.9, 1, 1.1, 1.25, 1.5, 1.75, 2];
const KEY = 'nebulosa.uiScale';

export const uiScale = $state({ value: load() });

function load(): number {
  try {
    const v = Number(localStorage.getItem(KEY));
    return SCALES.includes(v) ? v : 1;
  } catch {
    return 1;
  }
}

/** whether the app sets the scale itself (else: the browser's zoom) */
export const canScale = isNative;

export async function setScale(v: number) {
  v = SCALES.reduce((a, b) => (Math.abs(b - v) < Math.abs(a - v) ? b : a));
  uiScale.value = v;
  try {
    localStorage.setItem(KEY, String(v));
  } catch {
    /* storage unavailable */
  }
  if (!canScale) return;
  const { getCurrentWebview } = await import('@tauri-apps/api/webview');
  await getCurrentWebview().setZoom(v);
}

/** one step bigger (+1) or smaller (−1) */
export function stepScale(d: 1 | -1) {
  const k = SCALES.indexOf(uiScale.value);
  return setScale(SCALES[Math.max(0, Math.min(SCALES.length - 1, (k < 0 ? 3 : k) + d))]);
}

/** at startup: the saved scale */
export function restoreScale() {
  if (canScale && uiScale.value !== 1) return setScale(uiScale.value);
}


// ---- theme: dark (the default), light, or the system's -------------------------
export type Theme = 'dark' | 'light' | 'system';
const THEME_KEY = 'nebulosa.theme';

function loadTheme(): Theme {
  try {
    const v = new URLSearchParams(location.search).get('theme') ?? localStorage.getItem(THEME_KEY);
    return v === 'light' || v === 'system' ? v : 'dark';
  } catch {
    return 'dark';
  }
}

export const uiTheme = $state({ value: loadTheme() });

const systemDark = typeof matchMedia !== 'undefined' ? matchMedia('(prefers-color-scheme: dark)') : null;

/** data-theme on <html>: what the CSS reads (index.html sets it first, before the first paint) */
function applyTheme() {
  const t = uiTheme.value === 'system' ? (systemDark?.matches ? 'dark' : 'light') : uiTheme.value;
  document.documentElement.dataset.theme = t;
}

export function setTheme(t: Theme) {
  uiTheme.value = t;
  try {
    localStorage.setItem(THEME_KEY, t);
  } catch {
    /* storage unavailable */
  }
  applyTheme();
}

if (typeof document !== 'undefined') {
  applyTheme();
  systemDark?.addEventListener('change', () => uiTheme.value === 'system' && applyTheme());
}
