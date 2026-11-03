import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

// The UI lives in ui/. `npm run web` serves it to a browser (WebUSB);
// `npm run dev` runs it inside the Tauri shell.
export default defineConfig({
  root: 'ui',
  plugins: [svelte()],
  clearScreen: false,
  server: { port: 5173, strictPort: true },
  envPrefix: ['VITE_', 'TAURI_ENV_'],
  build: {
    outDir: '../dist',
    emptyOutDir: true,
    target: 'es2022',
  },
});
