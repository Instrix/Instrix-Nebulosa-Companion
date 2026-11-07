// The Debug tab's component, only in builds with it (NEBULOSA_DEBUG=1 or
// `vite --mode debug`): elsewhere __DEBUG_TAB__ is false, this is null, and
// the bundler leaves DebugView and debug.ts out of the build.
export const loadDebugView = __DEBUG_TAB__ ? () => import('./DebugView.svelte') : null;
