// The desktop backend, whichever shell runs the UI:
//   tauri:  Windows, macOS: Tauri commands and channels
//   ws:     Linux: the same backend (nebulosa-backend) behind a WebSocket on
//           localhost, the UI in a Chromium window (Electron, electron/)
//   web:    the browser: no backend (WebUSB / Web Serial, core in WASM)
// invoke() and channel() look the same in both desktop shells; the protocol
// is described in src-tauri/src/server.rs.

import { Channel as TauriChannel, invoke as tauriInvoke } from '@tauri-apps/api/core';

/** what Electron's preload (electron/preload.cjs) gives the page */
interface Shell {
  openFile(filters: { name: string; extensions: string[] }[]): Promise<string | null>;
  saveFile(defaultPath: string, filters: { name: string; extensions: string[] }[]): Promise<string | null>;
  setZoom(factor: number): void;
  onDropPaths(cb: (paths: string[]) => void): () => void;
}

const w = typeof window !== 'undefined' ? (window as unknown as { __TAURI_INTERNALS__?: unknown; nebulosaShell?: Shell }) : undefined;

/** the WebSocket shell's token: in the URL the backend printed, kept for reloads */
function wsToken(): string | null {
  if (!w) return null;
  try {
    const q = new URLSearchParams(location.search);
    const t = q.get('nbk');
    if (t) {
      sessionStorage.setItem('nebulosa.nbk', t);
      // out of the address (screenshots, copied links)
      q.delete('nbk');
      history.replaceState(null, '', location.pathname + (q.size ? `?${q}` : '') + location.hash);
      return t;
    }
    return sessionStorage.getItem('nebulosa.nbk');
  } catch {
    return null;
  }
}

/** a browser on the remote-control server (remote.rs), not the app's own window: ?remote=1, kept for reloads */
function remoteFlag(): boolean {
  if (!w) return false;
  try {
    const q = new URLSearchParams(location.search);
    if (q.get('remote') === '1') {
      sessionStorage.setItem('nebulosa.remote', '1');
      q.delete('remote');
      history.replaceState(null, '', location.pathname + (q.size ? `?${q}` : '') + location.hash);
    }
    return sessionStorage.getItem('nebulosa.remote') === '1';
  } catch {
    return false;
  }
}

export const shellKind: 'tauri' | 'ws' | 'web' = !w ? 'web' : '__TAURI_INTERNALS__' in w ? 'tauri' : wsToken() ? 'ws' : 'web';
/** this UI is a remote-control browser (it mirrors the app's window) */
export const isRemote = shellKind === 'ws' && remoteFlag();
/** the Electron shell's extras (dialogs, zoom, dropped files), if it is there */
export const shell: Shell | undefined = w?.nebulosaShell;

// ---- the WebSocket shell --------------------------------------------------------

/** a stream of events from one command (connect, open_recording, serial_open) */
export class WsChannel<T> {
  static next = 1;
  readonly id = WsChannel.next++;
  onmessage: (m: T) => void = () => undefined;
}

type Pending = { ok: (v: unknown) => void; err: (e: unknown) => void };
let sock: WebSocket | null = null;
let ready: Promise<WebSocket> | null = null;
let nextId = 1;
const pending = new Map<number, Pending>();
const channels = new Map<number, WsChannel<unknown>>();

function connectWs(): Promise<WebSocket> {
  if (ready) return ready;
  ready = new Promise((resolve, reject) => {
    const s = new WebSocket(`ws://${location.host}/ws?t=${encodeURIComponent(sessionStorage.getItem('nebulosa.nbk') ?? '')}`);
    s.binaryType = 'arraybuffer';
    s.onopen = () => {
      sock = s;
      resolve(s);
    };
    s.onerror = () => reject(new Error('The backend is not answering (WebSocket).'));
    s.onclose = () => {
      sock = null;
      ready = null;
      for (const p of pending.values()) p.err('The backend went away.');
      pending.clear();
    };
    s.onmessage = (e) => {
      if (typeof e.data === 'string') {
        const m = JSON.parse(e.data) as { id?: number; ok?: unknown; err?: string; ch?: number; msg?: unknown };
        if (m.ch != null) channels.get(m.ch)?.onmessage(m.msg);
        else if (m.id != null) {
          const p = pending.get(m.id);
          pending.delete(m.id);
          if (m.err != null) p?.err(m.err);
          else p?.ok(m.ok);
        }
        return;
      }
      const buf = e.data as ArrayBuffer;
      const v = new DataView(buf);
      const kind = v.getUint8(0), id = v.getUint32(1, true);
      const body = buf.slice(5);
      if (kind === 1) {
        const p = pending.get(id);
        pending.delete(id);
        p?.ok(body);
      } else if (kind === 2) channels.get(id)?.onmessage(new Uint8Array(body));
    };
  });
  return ready;
}

async function wsInvoke<T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
  // a channel among the arguments: its events come back under its id
  let ch: number | undefined;
  const plain: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(args)) {
    if (v instanceof WsChannel) {
      ch = v.id;
      channels.set(v.id, v as WsChannel<unknown>);
    } else plain[k] = v;
  }
  const s = sock ?? (await connectWs());
  const id = nextId++;
  return new Promise<T>((ok, err) => {
    pending.set(id, { ok: ok as (v: unknown) => void, err });
    s.send(JSON.stringify({ id, cmd, args: plain, ...(ch != null ? { ch } : {}) }));
  });
}

// ---- either shell -----------------------------------------------------------------

/** a backend command: its result (JSON, or an ArrayBuffer for the byte answers) */
export function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  return shellKind === 'ws' ? wsInvoke<T>(cmd, args) : tauriInvoke<T>(cmd, args);
}

/** a channel to pass to a command; `onmessage` gets its events */
export function channel<T>(onmessage: (m: T) => void): TauriChannel<T> | WsChannel<T> {
  const c = shellKind === 'ws' ? new WsChannel<T>() : new TauriChannel<T>();
  c.onmessage = onmessage;
  return c;
}

type Filter = { name: string; extensions: string[] };

/** a file to open, picked by the user */
export async function pickOpen(filters: Filter[]): Promise<string | null> {
  if (shellKind === 'ws') return shell ? shell.openFile(filters) : prompt('Path of the file to open');
  const { open } = await import('@tauri-apps/plugin-dialog');
  const p = await open({ multiple: false, directory: false, filters });
  return typeof p === 'string' ? p : null;
}

/** where to save a file, picked by the user */
export async function pickSave(defaultPath: string, filters: Filter[]): Promise<string | null> {
  if (shellKind === 'ws') return shell ? shell.saveFile(defaultPath, filters) : prompt('Save as', defaultPath);
  const { save } = await import('@tauri-apps/plugin-dialog');
  return save({ defaultPath, filters });
}

/** the window's zoom (the UI scale) */
export async function setZoom(factor: number) {
  if (shellKind === 'ws') return shell?.setZoom(factor);
  const { getCurrentWebview } = await import('@tauri-apps/api/webview');
  await getCurrentWebview().setZoom(factor);
}

/** files dropped on the window, by path; returns the unsubscribe */
export async function onDropPaths(cb: (paths: string[]) => void): Promise<() => void> {
  if (shellKind === 'ws') return shell?.onDropPaths(cb) ?? (() => undefined);
  const { getCurrentWebview } = await import('@tauri-apps/api/webview');
  return getCurrentWebview().onDragDropEvent((e) => {
    if (e.payload.type === 'drop') cb(e.payload.paths);
  });
}
