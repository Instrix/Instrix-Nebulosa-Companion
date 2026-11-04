// A theme colour (a CSS custom property on :root, app.css) for drawing on a
// canvas. Plots read several on every frame: getComputedStyle each time can
// force a style recalculation mid-frame, so the values are kept per theme
// (data-theme on <html>, a plain attribute read) and read again only when it
// changes.

let theme: string | undefined;
let cache = new Map<string, string>();

export function themeVar(name: string): string {
  const t = document.documentElement.dataset.theme ?? '';
  if (t !== theme) {
    theme = t;
    cache = new Map();
  }
  let v = cache.get(name);
  if (v === undefined) {
    v = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
    cache.set(name, v);
  }
  return v;
}
