// A label on a plot (serial messages, captures): a solid chip in the marker's
// colour, its text black or white, whichever reads better on that colour.

/** the colour as r, g, b (0..255), whatever CSS form it came in */
function rgb(ctx: CanvasRenderingContext2D, col: string): [number, number, number] {
  ctx.fillStyle = '#000';
  ctx.fillStyle = col;
  const c = String(ctx.fillStyle);
  if (c.startsWith('#')) return [1, 3, 5].map((i) => parseInt(c.slice(i, i + 2), 16)) as [number, number, number];
  const m = c.match(/[\d.]+/g) ?? ['0', '0', '0'];
  return [+m[0], +m[1], +m[2]];
}

/** black or white text, the one with more contrast on the colour */
export function inkOn(ctx: CanvasRenderingContext2D, col: string): string {
  const L = rgb(ctx, col)
    .map((v) => v / 255)
    .map((v) => (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4));
  const lum = 0.2126 * L[0] + 0.7152 * L[1] + 0.0722 * L[2];
  return (lum + 0.05) / 0.05 > 1.05 / (lum + 0.05) ? '#0b0c12' : '#ffffff';
}

/** the chip's font, in canvas pixels */
export const chipFont = (dpr: number) => `600 ${11 * dpr}px ui-monospace, monospace`;

/** draw a chip at x (its left edge), y (its top), all in canvas pixels; returns its width */
export function chip(ctx: CanvasRenderingContext2D, x: number, y: number, text: string, col: string, dpr: number): number {
  ctx.save();
  ctx.font = chipFont(dpr);
  ctx.textBaseline = 'middle';
  ctx.textAlign = 'left';
  const h = 15 * dpr, pad = 4 * dpr;
  const w = ctx.measureText(text).width + 2 * pad;
  ctx.globalAlpha = 1;
  ctx.fillStyle = col;
  ctx.fillRect(x, y, w, h);
  ctx.fillStyle = inkOn(ctx, col);
  ctx.fillText(text, x + pad, y + h / 2 + 0.5 * dpr);
  ctx.restore();
  return w;
}

/** a chip's width, to place it before drawing */
export function chipWidth(ctx: CanvasRenderingContext2D, text: string, dpr: number): number {
  ctx.save();
  ctx.font = chipFont(dpr);
  const w = ctx.measureText(text).width + 8 * dpr;
  ctx.restore();
  return w;
}
