// One bring-up for the session: it keeps its instruments, results and
// playbook while the Debug tab is closed and reopened (each opening hands it
// the tab's current props). ALL OFF when the window closes.
import { Bringup, type Host } from './engine.svelte';

let inst: Bringup | null = null;
let current: Host | null = null;

/** the bring-up, reading the app through `host` from now on */
export function bringup(host: Host): Bringup {
  current = host;
  if (!inst) {
    inst = new Bringup({
      dev: () => current?.dev() ?? null,
      streaming: () => current?.streaming() ?? false,
      startAdc: () => current?.startAdc() ?? Promise.resolve(),
      log: (t, bad) => current?.log(t, bad),
      demoBench: (b) => current?.demoBench?.(b),
    });
    addEventListener('beforeunload', () => void inst?.everythingOff());
  }
  return inst;
}
