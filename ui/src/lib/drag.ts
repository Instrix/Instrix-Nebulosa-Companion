// `use:drag={{ start, move, end }}`: pointer drags on an element (a splitter
// handle). `move` gets the distance from where the drag started, in CSS px.

export interface DragOpts {
  start?: () => void;
  move: (dx: number, dy: number) => void;
  end?: () => void;
}

export function drag(node: HTMLElement, opts: DragOpts) {
  let o = opts;
  let x0 = 0;
  let y0 = 0;
  const move = (e: PointerEvent) => o.move(e.clientX - x0, e.clientY - y0);
  const up = (e: PointerEvent) => {
    node.releasePointerCapture?.(e.pointerId);
    node.removeEventListener('pointermove', move);
    node.removeEventListener('pointerup', up);
    node.removeEventListener('pointercancel', up);
    o.end?.();
  };
  const down = (e: PointerEvent) => {
    if (e.button !== 0) return;
    e.preventDefault();
    x0 = e.clientX;
    y0 = e.clientY;
    node.setPointerCapture(e.pointerId);
    o.start?.();
    node.addEventListener('pointermove', move);
    node.addEventListener('pointerup', up);
    node.addEventListener('pointercancel', up);
  };
  node.addEventListener('pointerdown', down);
  return {
    update(n: DragOpts) {
      o = n;
    },
    destroy() {
      node.removeEventListener('pointerdown', down);
    },
  };
}

/** A number kept in localStorage (when there is one). */
export function stored(key: string, fallback: number, ok: (v: number) => boolean = () => true): number {
  try {
    const v = Number(localStorage.getItem(key));
    if (localStorage.getItem(key) != null && Number.isFinite(v) && ok(v)) return v;
  } catch {
    /* storage unavailable */
  }
  return fallback;
}

export function store(key: string, v: unknown) {
  try {
    localStorage.setItem(key, typeof v === 'string' ? v : JSON.stringify(v));
  } catch {
    /* storage unavailable */
  }
}
