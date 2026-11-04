// A view's back / forward history, like a browser's. It records the view each
// time it settles (held still for a moment): a jump (to a marker, an event,
// a message, Autoset…) and the end of every zoom or pan gesture is a step,
// without each of those having to say so. Back and forward put a step back
// on screen; moving on from there drops the steps ahead, as a browser does.

const SETTLE_MS = 600;
const MAX = 100;

export class ViewHistory<T> {
  private stack: T[] = [];
  private i = -1;
  private timer: ReturnType<typeof setTimeout> | undefined;
  private pending: T | null = null;
  canBack = $state(false);
  canForward = $state(false);

  constructor(
    /** two states show the same view */
    private same: (a: T, b: T) => boolean,
    /** put a state on screen */
    private apply: (s: T) => void,
  ) {}

  /** the view now: recorded once it has stayed so for a moment */
  observe(s: T) {
    this.pending = s;
    clearTimeout(this.timer);
    this.timer = setTimeout(() => this.commit(), SETTLE_MS);
  }

  private commit() {
    clearTimeout(this.timer);
    const s = this.pending;
    this.pending = null;
    if (!s || (this.i >= 0 && this.same(this.stack[this.i], s))) return;
    this.stack = this.stack.slice(0, this.i + 1);
    this.stack.push(s);
    if (this.stack.length > MAX) this.stack.shift();
    this.i = this.stack.length - 1;
    this.flags();
  }

  private flags() {
    this.canBack = this.i > 0;
    this.canForward = this.i < this.stack.length - 1;
  }

  back() {
    // a view not yet settled counts: back goes before it
    if (this.pending) this.commit();
    if (this.i <= 0) return false;
    this.i--;
    this.go();
    return true;
  }

  forward() {
    if (this.pending) this.commit();
    if (this.i >= this.stack.length - 1) return false;
    this.i++;
    this.go();
    return true;
  }

  private go() {
    this.flags();
    const s = this.stack[this.i];
    this.apply(s);
    // what that puts on screen is this step, not a new one
    this.pending = null;
    clearTimeout(this.timer);
  }

  clear() {
    this.stack = [];
    this.i = -1;
    this.pending = null;
    clearTimeout(this.timer);
    this.flags();
  }
}
