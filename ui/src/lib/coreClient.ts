// The UI's handle on the core host (coreHost.ts): in a Web Worker when one
// starts, so the stream's parsing and the buffer's queries run beside the UI,
// not on its thread; in the page otherwise. Either way: async calls by name.
// One host for the app: each link resets its pipeline and takes its events
// (the side-channel campaign carries over, as it always has).

import { initCore } from './nbp';
import { CoreHost, type HostEvent } from './coreHost';

/** how long a worker gets to load its WASM and answer before the page does it */
const START_MS = 8000;

export interface CoreClient {
  /** where the core runs */
  readonly where: 'worker' | 'page';
  call<T = unknown>(method: string, ...args: unknown[]): Promise<T>;
  /** no answer wanted (the stream's bytes: moved, not copied) */
  send(method: string, args: unknown[], transfer?: Transferable[]): void;
  /** `owner` is done: its timers stop (the host stays for the next) */
  release(owner: object): void;
}

class WorkerClient implements CoreClient {
  readonly where = 'worker';
  private next = 1;
  private waiting = new Map<number, { resolve: (v: unknown) => void; reject: (e: Error) => void }>();

  constructor(private w: Worker) {
    w.onmessage = (e: MessageEvent<{ id?: number; ok?: boolean; value?: unknown; error?: string; event?: HostEvent }>) => {
      const d = e.data;
      if (d.event) return listener.on(d.event);
      const p = this.waiting.get(d.id!);
      if (!p) return;
      this.waiting.delete(d.id!);
      if (d.ok) p.resolve(d.value);
      else p.reject(new Error(d.error));
    };
  }

  call<T>(method: string, ...args: unknown[]): Promise<T> {
    const id = this.next++;
    return new Promise<T>((resolve, reject) => {
      this.waiting.set(id, { resolve: resolve as (v: unknown) => void, reject });
      this.w.postMessage({ id, method, args });
    });
  }

  send(method: string, args: unknown[], transfer: Transferable[] = []) {
    this.w.postMessage({ id: 0, method, args }, { transfer });
  }

  release(owner: object) {
    if (listener.owner !== owner) return;
    listener.owner = null;
    listener.on = () => undefined;
    this.send('close', []);
  }

  terminate() {
    this.w.terminate();
    for (const p of this.waiting.values()) p.reject(new Error('closed'));
    this.waiting.clear();
  }
}

class PageClient implements CoreClient {
  readonly where = 'page';
  constructor(private host: CoreHost) {}
  async call<T>(method: string, ...args: unknown[]): Promise<T> {
    return (await this.host.handle(method, args)) as T;
  }
  send(method: string, args: unknown[]) {
    try {
      this.host.handle(method, args);
    } catch (e) {
      console.warn('core', method, e);
    }
  }
  release(owner: object) {
    if (listener.owner !== owner) return;
    listener.owner = null;
    listener.on = () => undefined;
    this.host.handle('close', []);
  }
}

async function startWorker(): Promise<CoreClient> {
  const w = new Worker(new URL('./pipeline.worker.ts', import.meta.url), { type: 'module', name: 'nebulosa-core' });
  const c = new WorkerClient(w);
  let to: ReturnType<typeof setTimeout> | undefined;
  try {
    await Promise.race([
      c.call('ping'),
      new Promise((_, reject) => {
        w.onerror = (e) => reject(new Error(e.message || 'worker failed'));
        to = setTimeout(() => reject(new Error('worker did not start')), START_MS);
      }),
    ]);
    w.onerror = null;
    return c;
  } catch (e) {
    c.terminate();
    throw e;
  } finally {
    clearTimeout(to);
  }
}

/** who gets the host's events now: the newest link */
const listener: { owner: object | null; on: (e: HostEvent) => void } = { owner: null, on: () => undefined };
let shared: Promise<CoreClient> | null = null;

async function start(): Promise<CoreClient> {
  // `?core=page` keeps it in the page (comparing, debugging)
  const inPage = typeof location !== 'undefined' && new URLSearchParams(location.search).get('core') === 'page';
  if (!inPage && typeof Worker !== 'undefined') {
    try {
      return await startWorker();
    } catch (e) {
      console.warn('core worker unavailable, running in the page:', e);
    }
  }
  await initCore();
  return new PageClient(new CoreHost((e) => listener.on(e)));
}

/** the core host for a new link `owner`: a fresh pipeline, its events to `onEvent` */
export async function openCore(owner: object, onEvent: (e: HostEvent) => void): Promise<CoreClient> {
  shared ??= start();
  const c = await shared;
  listener.owner = owner;
  listener.on = onEvent;
  await c.call('reset');
  return c;
}
