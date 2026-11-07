// Remote control (src-tauri/src/remote.rs): the Companion in a browser,
// mirroring the app's window on the same backend, and a JSON API for scripts.
//
//   - the server: on / off, its port, this computer only or the local network,
//     its token (kept, so a bookmarked link keeps working)
//   - sync: the UIs on the backend tell each other what changed (the
//     instrument's settings, streaming, a device opened or closed)
//   - the API: the app's window carries out /api/v1/<action> calls
//
// The actions (GET or POST, arguments as a JSON body or the query; the token
// as ?t=… or "Authorization: Bearer …"), handled in App.svelte:
//   status, devices, connect {key | host [port]}, demo, disconnect, start, stop,
//   measure {seconds}, value, settings, set {acquisition, trigger, logger},
//   markers, marker {t0 [t1] [name]}, clear_markers, record {on}, log {on},
//   mode {mode}, screenshot

import { channel, invoke } from './ipc';

export interface RemoteStatus {
  running: boolean;
  port: number;
  lan: boolean;
  token: string;
  urls: string[];
  peers: number;
}

export interface RemoteCfg {
  on: boolean;
  port: number;
  lan: boolean;
  token: string;
}

export const DEFAULT_PORT = 8765;
const KEY = 'nebulosa.remote';

export function loadRemoteCfg(): RemoteCfg {
  const d: RemoteCfg = { on: false, port: DEFAULT_PORT, lan: false, token: '' };
  try {
    return { ...d, ...(JSON.parse(localStorage.getItem(KEY) ?? '{}') as Partial<RemoteCfg>) };
  } catch {
    return d;
  }
}

export function saveRemoteCfg(c: RemoteCfg) {
  try {
    localStorage.setItem(KEY, JSON.stringify(c));
  } catch {
    /* storage unavailable */
  }
}

export const remoteStatus = () => invoke<RemoteStatus>('remote_status');
export const remoteStop = () => invoke<RemoteStatus>('remote_stop');

/** starts the server as `c` says; the token it chose is kept */
export async function remoteStart(c: RemoteCfg): Promise<RemoteStatus> {
  const s = await invoke<RemoteStatus>('remote_start', { port: c.port, lan: c.lan, token: c.token || null });
  if (s.token !== c.token) saveRemoteCfg({ ...c, token: s.token });
  return s;
}

// ---- sync between the UIs ---------------------------------------------------------------

export type SyncMsg =
  /** a UI just joined: the others send their state */
  | { t: 'hello' }
  /** the instrument's settings, and whether it streams */
  | { t: 'state'; settings: Record<string, unknown>; streaming: boolean; rateHz: number }
  /** a device, recording or log was opened: join it */
  | { t: 'opened' }
  /** it was closed */
  | { t: 'closed' };

export class Sync {
  private id = 0;

  async join(on: (m: SyncMsg) => void): Promise<void> {
    this.id = await invoke<number>('sync_join', { events: channel<{ from: number; msg: SyncMsg }>((m) => on(m.msg)) });
  }

  get joined(): boolean {
    return this.id > 0;
  }

  send(msg: SyncMsg) {
    if (this.id) invoke<void>('sync_send', { from: this.id, msg }).catch(() => undefined);
  }

  leave() {
    if (this.id) invoke<void>('sync_leave', { id: this.id }).catch(() => undefined);
    this.id = 0;
  }
}

// ---- the API's calls, carried out here --------------------------------------------------

export type ApiHandler = (args: Record<string, unknown>) => unknown | Promise<unknown>;

/** this UI answers the HTTP API (the app's window) */
export async function hostApi(handlers: Record<string, ApiHandler>): Promise<void> {
  await invoke<void>('api_host', {
    events: channel<{ id: number; action: string; args: unknown }>(async ({ id, action, args }) => {
      let ok: unknown = null;
      let err: string | null = null;
      try {
        const h = handlers[action];
        if (!h) throw new Error(`unknown action "${action}": ${Object.keys(handlers).join(', ')}`);
        ok = (await h((args && typeof args === 'object' ? args : {}) as Record<string, unknown>)) ?? null;
      } catch (e) {
        err = e instanceof Error ? e.message : String(e);
      }
      invoke<void>('api_reply', { id, ok, err }).catch(() => undefined);
    }),
  });
}

/** "Bearer" example for the docs: a curl line */
export function curlExample(s: RemoteStatus, action = 'status'): string {
  return `curl -H "Authorization: Bearer ${s.token}" http://127.0.0.1:${s.port}/api/v1/${action}`;
}
