// The wheel by distance, not by event: a mouse notch (~100 px) is one step,
// a trackpad's stream of small deltas adds up to steps.

/** a wheel event's distance in px */
export function wheelPx(e: WheelEvent, d: number): number {
  return d * (e.deltaMode === 1 ? 16 : e.deltaMode === 2 ? 400 : 1);
}

/** Turns wheel events into whole zoom steps (+ out, − in). */
export function zoomStepper(stepPx = 60) {
  let acc = 0;
  let last = 0;
  return (e: WheelEvent): number => {
    const d = wheelPx(e, e.deltaY);
    const now = performance.now();
    // a pause, or the other way: start counting again
    if (now - last > 250 || Math.sign(d) !== Math.sign(acc)) acc = 0;
    last = now;
    acc += d;
    // a mouse notch always moves at least one step
    if (Math.abs(d) >= stepPx * 0.8 && Math.abs(acc) < stepPx) acc = Math.sign(acc) * stepPx;
    const steps = Math.trunc(acc / stepPx);
    acc -= steps * stepPx;
    return steps;
  };
}
