import { mount } from 'svelte';
import './app.css';
import App from './App.svelte';
import { initCore } from './lib/nbp';

// the splash (index.html) stays until the app is up: at least this long, so it
// doesn't just flash by
const SPLASH_MIN_MS = 900;
const t0 = performance.now();
const splash = document.getElementById('splash');

function splashDone() {
  if (!splash) return;
  const wait = Math.max(0, SPLASH_MIN_MS - (performance.now() - t0));
  setTimeout(() => {
    splash.classList.add('done');
    splash.addEventListener('transitionend', () => splash.remove(), { once: true });
    setTimeout(() => splash.remove(), 1000); // no transition (reduced motion, hidden tab)
  }, wait);
}

let app: ReturnType<typeof mount> | undefined;
try {
  // The Rust core (WASM) parses and encodes NBP for both builds: load it first.
  await initCore();
  app = mount(App, { target: document.getElementById('app')! });
  splashDone();
} catch (e) {
  splash?.classList.add('failed');
  const msg = splash?.querySelector('.msg');
  if (msg) msg.textContent = `Could not start: ${e instanceof Error ? e.message : String(e)}`;
  throw e;
}

export default app;
