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
  const { setZoom } = await import('./ipc');
  await setZoom(v);
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


// ---- theme ---------------------------------------------------------------------
// Each theme is a base scheme (data-theme: dark or light, which the CSS and the
// logos key on) and, for the dark variants, a palette on top (data-palette,
// app.css). "system" follows the OS: light or dark, and high contrast when the
// OS asks for more contrast.

export type Theme = 'dark' | 'charcoal' | 'oled' | 'contrast' | 'light' | 'system';

export interface ThemeInfo {
  id: Theme;
  label: string;
  detail: string;
  /** for the settings' preview: ground, panel, border, text, accent */
  swatch: [string, string, string, string, string];
}

export const THEMES: ThemeInfo[] = [
  { id: 'dark', label: 'Dark', detail: 'Blue-tinted charcoal, the INSTRIX default', swatch: ['#161826', '#1d2030', '#2c3044', '#e9e9ed', '#2f80c0'] },
  { id: 'charcoal', label: 'Charcoal', detail: 'Neutral grey, no tint', swatch: ['#18181a', '#222225', '#36363c', '#e8e8ea', '#3a8fd0'] },
  { id: 'oled', label: 'OLED black', detail: 'Pure black ground: dark rooms, OLED screens', swatch: ['#000000', '#0b0b0d', '#222329', '#e6e6ea', '#2f80c0'] },
  { id: 'contrast', label: 'High contrast', detail: 'Black, white and bright edges: the most legible', swatch: ['#000000', '#000000', '#c9ced8', '#ffffff', '#1a78cc'] },
  { id: 'light', label: 'Light', detail: 'Paper white', swatch: ['#f6f7f9', '#ffffff', '#e3e6eb', '#292b31', '#23699f'] },
  { id: 'system', label: "The system's", detail: 'Light or dark (and high contrast) as the OS is set', swatch: ['#f6f7f9', '#1d2030', '#2c3044', '#e9e9ed', '#2f80c0'] },
];

const THEME_KEY = 'nebulosa.theme';

export const isTheme = (v: unknown): v is Theme => THEMES.some((t) => t.id === v);

function loadTheme(): Theme {
  try {
    const v = new URLSearchParams(location.search).get('theme') ?? localStorage.getItem(THEME_KEY);
    return isTheme(v) ? v : 'dark';
  } catch {
    return 'dark';
  }
}

export const uiTheme = $state({ value: loadTheme() });

const mq = (q: string) => (typeof matchMedia !== 'undefined' ? matchMedia(q) : null);
const systemDark = mq('(prefers-color-scheme: dark)');
const systemMore = mq('(prefers-contrast: more)');

/** data-theme (and data-palette) on <html>: what the CSS reads (index.html sets them first, before the first paint) */
function applyTheme() {
  const t: Exclude<Theme, 'system'> = uiTheme.value === 'system' ? (systemMore?.matches ? 'contrast' : systemDark?.matches ? 'dark' : 'light') : uiTheme.value;
  const root = document.documentElement;
  root.dataset.theme = t === 'light' ? 'light' : 'dark';
  if (t === 'light' || t === 'dark') delete root.dataset.palette;
  else root.dataset.palette = t;
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
  const follow = () => uiTheme.value === 'system' && applyTheme();
  systemDark?.addEventListener('change', follow);
  systemMore?.addEventListener('change', follow);
}
