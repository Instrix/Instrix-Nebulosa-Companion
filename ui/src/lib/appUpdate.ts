// Is there a newer Nebulosa Companion? The latest release on GitHub
// (Instrix/Instrix-Nebulosa-Companion: tags vX.Y.Z, built by
// .github/workflows/release.yml) against this build's version. Checked once
// on boot in the desktop app (Settings can turn that off) and by hand from
// Help → Check for updates. Nothing is downloaded or installed here: the
// release page or the right installer opens in the browser.

import { invoke, shellKind } from './ipc';

export const REPO = 'Instrix/Instrix-Nebulosa-Companion';
export const RELEASES_URL = `https://github.com/${REPO}/releases`;
const API_LATEST = `https://api.github.com/repos/${REPO}/releases/latest`;

const AUTO_KEY = 'nebulosa.updateCheck';
const SKIP_KEY = 'nebulosa.updateSkip';

export interface Asset {
  name: string;
  url: string;
  size: number;
}

export interface Release {
  version: string;
  tag: string;
  url: string;
  notes: string;
  published: string;
  assets: Asset[];
}

export interface UpdateCheck {
  current: string;
  latest: Release;
  newer: boolean;
}

/** "v1.2.3-rc.1" -> [1, 2, 3] and the pre-release part */
function parse(v: string): { n: number[]; pre: string } {
  const [core, pre = ''] = v.trim().replace(/^v/i, '').split(/[-+]/, 2);
  return { n: core.split('.').map((x) => parseInt(x, 10) || 0), pre };
}

/** > 0 when a is newer than b (a pre-release is older than its release) */
export function compareVersions(a: string, b: string): number {
  const x = parse(a);
  const y = parse(b);
  for (let i = 0; i < Math.max(x.n.length, y.n.length); i++) {
    const d = (x.n[i] ?? 0) - (y.n[i] ?? 0);
    if (d) return d;
  }
  if (x.pre === y.pre) return 0;
  if (!x.pre) return 1;
  if (!y.pre) return -1;
  return x.pre < y.pre ? -1 : 1;
}

/** the latest published release (not drafts or pre-releases); throws when GitHub can't be reached */
export async function checkForUpdate(timeoutMs = 10000): Promise<UpdateCheck> {
  const ctl = new AbortController();
  const timer = setTimeout(() => ctl.abort(), timeoutMs);
  try {
    const r = await fetch(API_LATEST, { headers: { Accept: 'application/vnd.github+json' }, signal: ctl.signal, cache: 'no-store' });
    if (r.status === 404) throw new Error('no release published yet');
    if (r.status === 403 || r.status === 429) throw new Error('GitHub is limiting requests from this address: try again later');
    if (!r.ok) throw new Error(`GitHub answered ${r.status}`);
    const d = await r.json();
    const latest: Release = {
      version: String(d.tag_name ?? '').replace(/^v/i, ''),
      tag: String(d.tag_name ?? ''),
      url: String(d.html_url ?? RELEASES_URL),
      notes: String(d.body ?? ''),
      published: String(d.published_at ?? ''),
      assets: (Array.isArray(d.assets) ? d.assets : []).map((a: { name: string; browser_download_url: string; size: number }) => ({ name: a.name, url: a.browser_download_url, size: a.size })),
    };
    return { current: __APP_VERSION__, latest, newer: compareVersions(latest.version, __APP_VERSION__) > 0 };
  } catch (e) {
    if ((e as Error).name === 'AbortError') throw new Error('GitHub did not answer in time');
    if (e instanceof TypeError) throw new Error('no connection to GitHub');
    throw e;
  } finally {
    clearTimeout(timer);
  }
}

// ---- which download suits this computer ---------------------------------------------

export type Os = 'windows' | 'macos' | 'linux' | 'other';

export function thisOs(): Os {
  const p = `${navigator.platform} ${navigator.userAgent}`.toLowerCase();
  if (p.includes('win')) return 'windows';
  if (p.includes('mac')) return 'macos';
  if (p.includes('linux')) return 'linux';
  return 'other';
}

const isArm = () => /arm|aarch64/i.test(`${navigator.platform} ${navigator.userAgent}`);

/** this OS's downloads, the likeliest first (installer, this architecture) */
export function assetsFor(assets: Asset[], os = thisOs()): Asset[] {
  const ext: Record<Os, RegExp> = {
    windows: /(setup\.exe|\.msi|portable\.zip)$/i,
    macos: /(\.dmg|\.app\.zip)$/i,
    linux: /(\.AppImage|\.deb|\.rpm)$/i,
    other: /$^/,
  };
  const arm = isArm();
  const rank = (n: string) => {
    let s = 0;
    if (/(arm64|aarch64)/i.test(n) !== arm && !/universal/i.test(n)) s += 10;
    if (os === 'windows') s += /setup\.exe$/i.test(n) ? 0 : /\.msi$/i.test(n) ? 1 : 2;
    if (os === 'macos') s += /\.dmg$/i.test(n) ? 0 : 1;
    if (os === 'linux') s += /\.AppImage$/i.test(n) ? 0 : /\.deb$/i.test(n) ? 1 : 2;
    return s;
  };
  // each package is there twice (release.yml): with the version in its name, and without (for the
  // releases/latest/download links); one of each is enough
  const seen = new Set<string>();
  return assets
    .filter((a) => ext[os].test(a.name))
    .sort((a, b) => rank(a.name) - rank(b.name) || b.name.length - a.name.length)
    .filter((a) => {
      const k = a.name.replace(/^(nebulosa-companion)_\d+\.\d+\.\d+[^_]*_/, '$1_');
      if (seen.has(k)) return false;
      seen.add(k);
      return true;
    });
}

// ---- preferences ----------------------------------------------------------------------

/** the check on boot: on by default in the desktop app (a browser gets the page's version anyway) */
export function autoCheck(): boolean {
  try {
    const v = localStorage.getItem(AUTO_KEY);
    return v === null ? shellKind !== 'web' : v === '1';
  } catch {
    return shellKind !== 'web';
  }
}

export function setAutoCheck(on: boolean) {
  try {
    localStorage.setItem(AUTO_KEY, on ? '1' : '0');
  } catch {
    /* storage unavailable */
  }
}

/** a version the boot check stays quiet about */
export function skippedVersion(): string {
  try {
    return localStorage.getItem(SKIP_KEY) ?? '';
  } catch {
    return '';
  }
}

export function skipVersion(v: string) {
  try {
    localStorage.setItem(SKIP_KEY, v);
  } catch {
    /* storage unavailable */
  }
}

/** a web page in the system's browser */
export async function openExternal(url: string) {
  if (shellKind === 'tauri') await invoke<void>('open_url', { url });
  else window.open(url, '_blank', 'noopener'); // Electron hands it to the browser (electron/main.cjs)
}
