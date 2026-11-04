// The core host (coreHost.ts) in a Web Worker: its own WASM instance, the
// pipeline, the campaign, the demo generator. Calls come in as
// { id, method, args }; answers go back as { id, ok, value } (id 0: no answer
// wanted), events as { event }.

import { initCore } from './nbp';
import { CoreHost } from './coreHost';

const ready = initCore();
let host: CoreHost | null = null;

/** typed arrays' buffers go back moved, not copied */
function transferables(v: unknown): Transferable[] {
  return ArrayBuffer.isView(v) && v.buffer instanceof ArrayBuffer ? [v.buffer] : [];
}

self.onmessage = async (e: MessageEvent<{ id: number; method: string; args: unknown[] }>) => {
  const { id, method, args } = e.data;
  try {
    await ready;
    host ??= new CoreHost((event) => self.postMessage({ event }));
    const value = await host.handle(method, args);
    if (id) self.postMessage({ id, ok: true, value }, { transfer: transferables(value) });
  } catch (err) {
    if (id) self.postMessage({ id, ok: false, error: err instanceof Error ? err.message : String(err) });
    else console.warn('core worker', method, err);
  }
};
