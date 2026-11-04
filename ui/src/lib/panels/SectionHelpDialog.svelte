<script lang="ts">
  // A section's "?": a live snapshot of the section (a copy of it as it is now,
  // values included, not clickable) with each option boxed and numbered, and an
  // arrow from its description to it. A copy rather than a picture: it always
  // matches the app, the theme and the device connected.
  import Icon from './Icon.svelte';
  import type { SectionHelp } from './sectionHelp';

  let {
    sec,
    id,
    title,
    icon = '',
    help,
    onclose,
  }: {
    /** the section element to copy */
    sec: HTMLElement;
    /** its id: a panel can show all its options for the snapshot (nebulosa:help-preview) */
    id: string;
    title: string;
    icon?: string;
    help: SectionHelp;
    onclose: () => void;
  } = $props();

  const LISTS = new Set(['DL', 'UL', 'OL', 'TABLE', 'TBODY']);
  const GAP = 64; // between the snapshot and the descriptions: room for the arrows
  const CARD_W = 300;

  let stage = $state<HTMLDivElement>();
  let snapW = $state(320);
  let boxes = $state<({ x: number; y: number; w: number; h: number } | null)[]>([]);
  let tops = $state<number[]>([]);
  /** which side of the snapshot each description is on */
  let sides = $state<('l' | 'r')[]>([]);
  /** the snapshot's left edge: left column, gap */
  const SNAP_X = CARD_W + GAP;
  const rightX = $derived(SNAP_X + snapW + GAP);
  let stageH = $state(0);
  let hot = $state<number | null>(null);
  let ready = $state(false);
  let cardEls: HTMLDivElement[] = $state([]);
  let snapRoot: HTMLElement | null = null;

  /** the section copied, inside copies of its ancestors (scoped styles need them) */
  function buildSnapshot(host: HTMLElement) {
    const copy = sec.cloneNode(true) as HTMLElement;
    // values live in properties, not attributes: copy them over
    const from = sec.querySelectorAll<HTMLInputElement | HTMLSelectElement | HTMLTextAreaElement>('input, select, textarea');
    const to = copy.querySelectorAll<HTMLInputElement | HTMLSelectElement | HTMLTextAreaElement>('input, select, textarea');
    from.forEach((el, k) => {
      const c = to[k];
      if (!c) return;
      if (el instanceof HTMLInputElement && (el.type === 'checkbox' || el.type === 'radio')) (c as HTMLInputElement).checked = el.checked;
      else c.value = el.value;
    });
    // shown open, with everything folded inside it unfolded too
    copy.classList.add('open');
    copy.querySelectorAll<HTMLElement>('.body[hidden]').forEach((b) => b.removeAttribute('hidden'));
    copy.querySelectorAll('details').forEach((d) => (d.open = true));
    copy.querySelectorAll('.help').forEach((b) => b.remove());
    copy.querySelectorAll('[id], [data-tour], [data-sec]').forEach((e) => {
      e.removeAttribute('id');
      e.removeAttribute('data-tour');
      e.removeAttribute('data-sec');
    });
    copy.removeAttribute('data-tour');
    copy.removeAttribute('data-sec');
    let inner: HTMLElement = copy;
    for (let a = sec.parentElement; a && a !== document.body; a = a.parentElement) {
      const w = document.createElement(a.tagName);
      w.className = a.className;
      w.style.cssText =
        'display:block;position:static;overflow:visible;width:auto;height:auto;min-height:0;max-height:none;margin:0;padding:0;border:0;background:transparent;transform:none;box-shadow:none;';
      w.appendChild(inner);
      inner = w;
    }
    host.appendChild(inner);
    snapRoot = copy;
  }

  /** an option's element in the copy: "$selector", "=exact text", "~contained text", or a label's start */
  function find(needle: string): HTMLElement | null {
    if (!snapRoot) return null;
    if (needle.startsWith('$')) return snapRoot.querySelector<HTMLElement>(needle.slice(1));
    const contains = needle.startsWith('~');
    const exact = needle.startsWith('=');
    const n = (contains || exact ? needle.slice(1) : needle).toLowerCase();
    const body = snapRoot.querySelector<HTMLElement>('.body') ?? snapRoot;
    // in document order a parent comes first: the first match small enough to be
    // one control (not a whole group) is the outermost such element
    for (const el of body.querySelectorAll<HTMLElement>('*')) {
      // an option is in its select; a list holds several rows (its row is the option)
      if (el.tagName === 'OPTION' || el.closest('option') || LISTS.has(el.tagName)) continue;
      const t = (el.textContent ?? '').replace(/\s+/g, ' ').trim().toLowerCase();
      if (!(exact ? t === n : contains ? t.includes(n) : t.startsWith(n))) continue;
      const r = el.getBoundingClientRect();
      if (r.width && r.height && r.height <= 96) return el;
    }
    return null;
  }

  function measure() {
    if (!stage || !snapRoot) return;
    const s = stage.getBoundingClientRect();
    boxes = help.items.map((it) => {
      const needles = it.at == null ? [] : Array.isArray(it.at) ? it.at : [it.at];
      let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
      for (const n of needles) {
        const el = find(n);
        if (!el) continue;
        // a term in a list: the row, its value with it
        const els = [el];
        if (el.tagName === 'DT' && el.nextElementSibling?.tagName === 'DD') els.push(el.nextElementSibling as HTMLElement);
        for (const e of els) {
          const r = e.getBoundingClientRect();
          x0 = Math.min(x0, r.left);
          y0 = Math.min(y0, r.top);
          x1 = Math.max(x1, r.right);
          y1 = Math.max(y1, r.bottom);
        }
      }
      if (x0 === Infinity) return null;
      return { x: x0 - s.left - 3, y: y0 - s.top - 3, w: x1 - x0 + 6, h: y1 - y0 + 6 };
    });
    // descriptions next to what they describe, in the order things appear, on
    // whichever side lets each sit closest to its target
    const order = help.items.map((_, k) => k).sort((a, b) => (boxes[a]?.y ?? 1e9) - (boxes[b]?.y ?? 1e9));
    const t: number[] = [];
    const sd: ('l' | 'r')[] = [];
    const bottom = { l: 0, r: 0 };
    for (const k of order) {
      const b = boxes[k];
      const want = b ? b.y + b.h / 2 - 16 - (fromAbove(b) ? 40 : 0) : Math.min(bottom.l, bottom.r);
      // a narrow target (a button in a row) goes to its nearer side; the rest
      // to whichever side has room closest to it
      // a small control at an edge: that side, the arrow straight in; else
      // whichever side has room nearest to it
      const side: 'l' | 'r' =
        b && narrow(b) && atEdge(b, 'l')
          ? 'l'
          : b && narrow(b) && atEdge(b, 'r')
            ? 'r'
            : Math.max(want, bottom.r) <= Math.max(want, bottom.l)
              ? 'r'
              : 'l';
      const top = Math.max(want, bottom[side]);
      t[k] = top;
      sd[k] = side;
      bottom[side] = top + (cardEls[k]?.offsetHeight ?? 60) + 8;
    }
    sides = sd;
    tops = t;
    stageH = Math.max(snapRoot.getBoundingClientRect().bottom - s.top, bottom.l, bottom.r);
    ready = true;
  }

  function snapshot(host: HTMLElement) {
    snapW = Math.round(sec.getBoundingClientRect().width) || 320;
    // let the panel show the options its current mode hides, copy it, then back
    dispatchEvent(new CustomEvent('nebulosa:help-preview', { detail: id }));
    requestAnimationFrame(() => {
      buildSnapshot(host);
      dispatchEvent(new CustomEvent('nebulosa:help-preview', { detail: null }));
      // after layout (twice: the cards' heights settle once their width is set)
      requestAnimationFrame(() => {
        measure();
        requestAnimationFrame(measure);
      });
    });
    const ro = new ResizeObserver(() => measure());
    ro.observe(host);
    return { destroy: () => ro.disconnect() };
  }

  type Box = { x: number; y: number; w: number; h: number };
  const narrow = (b: Box) => b.w < snapW * 0.6;
  /** nothing between the box and that side of the snapshot: an arrow can come straight in */
  const atEdge = (b: Box, side: 'l' | 'r') => (side === 'l' ? b.x - SNAP_X : SNAP_X + snapW - (b.x + b.w)) < 16;
  /** a small control mid-row: the arrow comes from above, not across its neighbours */
  const fromAbove = (b: Box) => narrow(b) && !atEdge(b, 'l') && !atEdge(b, 'r');

  /** the arrow from description k to its box: from the card's edge facing the snapshot */
  function arrow(k: number) {
    const b = boxes[k];
    if (!b) return '';
    const top = tops[k] ?? 0;
    const h = cardEls[k]?.offsetHeight ?? 40;
    const mid = b.y + b.h / 2;
    const y1 = Math.min(Math.max(mid, top + 14), top + h - 14);
    const left = sides[k] === 'l';
    const x1 = left ? CARD_W : rightX;
    if (fromAbove(b)) {
      // in from above: a side arrow would cross the controls next to it
      const x2 = b.x + b.w / 2;
      const y2 = b.y - 2;
      const dx = Math.max(20, Math.abs(x1 - x2) * 0.5) * (left ? 1 : -1);
      return `M ${x1} ${y1} C ${x1 + dx} ${y1}, ${x2} ${y2 - 36}, ${x2} ${y2}`;
    }
    const x2 = left ? b.x - 2 : b.x + b.w + 2;
    const y2 = mid;
    const dx = Math.max(20, Math.abs(x1 - x2) * 0.45) * (left ? 1 : -1);
    return `M ${x1} ${y1} C ${x1 + dx} ${y1}, ${x2 - dx} ${y2}, ${x2} ${y2}`;
  }

  function portal(node: HTMLElement) {
    document.body.appendChild(node);
    return { destroy: () => node.remove() };
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.stopPropagation();
      e.preventDefault();
      onclose();
    }
  }
</script>

<svelte:window onkeydowncapture={onKey} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<div class="hbackdrop" role="presentation" use:portal onclick={onclose}>
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div
    class="hdialog"
    role="dialog"
    aria-label="Help: {title}"
    tabindex="-1"
    style="width: min({snapW + 2 * (GAP + CARD_W) + 56}px, 96vw)"
    onclick={(e) => e.stopPropagation()}
  >
    <header>
      {#if icon}<Icon name={icon} />{/if}
      <h3>{title}</h3>
      <span class="muted small">— what's here</span>
      <button class="ghost small hclose" onclick={onclose} aria-label="Close">✕</button>
    </header>
    <p class="intro">{help.intro}</p>

    <div class="scroll">
      <div class="stage" bind:this={stage} style="height: {stageH}px; width: {snapW + 2 * (GAP + CARD_W)}px" class:ready>
        <div class="snap" style="left: {SNAP_X}px; width: {snapW}px" inert use:snapshot></div>

        {#each boxes as b, k (k)}
          {#if b}
            <div class="box" class:hot={hot === k} class:l={sides[k] === 'l'} style="left: {b.x}px; top: {b.y}px; width: {b.w}px; height: {b.h}px">
              <span class="num">{k + 1}</span>
            </div>
          {/if}
        {/each}

        <svg class="arrows" width={rightX} height={stageH} aria-hidden="true">
          <defs>
            <marker id="sh-head" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
              <path d="M 0 0 L 10 5 L 0 10 z" fill="context-stroke" />
            </marker>
          </defs>
          {#each boxes as b, k (k)}
            {#if b}
              <path d={arrow(k)} class:hot={hot === k} marker-end="url(#sh-head)" />
            {/if}
          {/each}
        </svg>

        {#each help.items as it, k (it.name)}
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div
            class="card"
            class:hot={hot === k}
            class:loose={!boxes[k]}
            class:l={sides[k] === 'l'}
            bind:this={cardEls[k]}
            style="left: {sides[k] === 'l' ? 0 : rightX}px; top: {tops[k] ?? 0}px; width: {CARD_W}px"
            onmouseenter={() => (hot = k)}
            onmouseleave={() => (hot = null)}
          >
            <span class="num">{k + 1}</span>
            <div>
              <b>{it.name}</b>
              <p>{it.text}</p>
              {#if it.list}
                <dl class="sub">
                  {#each it.list as [k, v] (k)}<dt>{k}</dt><dd>{v}</dd>{/each}
                </dl>
              {/if}
            </div>
          </div>
        {/each}
      </div>

      {#if help.tips?.length}
        <h4>How to use</h4>
        <ul>
          {#each help.tips as t (t)}<li>{t}</li>{/each}
        </ul>
      {/if}
      <p class="more muted">Unnumbered options aren't showing right now (they appear with other settings). More in Help (? or F1) and the user manual.</p>
    </div>
  </div>
</div>

<style>
  .hbackdrop {
    position: fixed;
    inset: 0;
    background: #000a;
    display: grid;
    place-items: center;
    z-index: 70;
  }
  .hdialog {
    max-height: 92vh;
    display: flex;
    flex-direction: column;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: var(--r-lg);
    padding: 14px 18px 12px;
    color: var(--text);
    font-size: 13px;
    line-height: 1.45;
    box-shadow: 0 20px 60px #0008;
  }
  header {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  header :global(.icon) {
    color: var(--accent);
  }
  h3 {
    margin: 0;
    font-size: 16px;
  }
  .hclose {
    margin-left: auto;
  }
  .intro {
    margin: 6px 0 12px;
  }
  .scroll {
    overflow-y: auto;
    overflow-x: auto;
    scrollbar-gutter: stable;
    min-height: 0;
    padding-right: 4px;
  }
  .stage {
    position: relative;
    opacity: 0;
    transition: opacity 0.15s;
  }
  .stage.ready {
    opacity: 1;
  }
  .snap {
    position: absolute;
    top: 8px;
    /* the section's own width: a frame outside it, not padding inside */
    outline: 1px dashed var(--border);
    outline-offset: 7px;
    border-radius: var(--r);
    background: var(--bg);
    box-shadow: 0 0 0 7px var(--bg);
    pointer-events: none;
    user-select: none;
    box-sizing: border-box;
  }
  .box {
    position: absolute;
    border: 2px solid var(--accent);
    border-radius: 5px;
    background: color-mix(in srgb, var(--accent) 8%, transparent);
    pointer-events: none;
    transition: background 0.12s, box-shadow 0.12s;
  }
  .box.hot {
    background: color-mix(in srgb, var(--accent) 22%, transparent);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 35%, transparent);
  }
  .box .num {
    position: absolute;
    right: -9px;
    top: -9px;
  }
  .box.l .num {
    right: auto;
    left: -9px;
  }
  .num {
    display: inline-grid;
    place-items: center;
    flex: none;
    width: 18px;
    height: 18px;
    border-radius: 50%;
    background: var(--accent);
    color: #fff;
    font-size: 11px;
    font-weight: 700;
    line-height: 1;
  }
  .arrows {
    position: absolute;
    left: 0;
    top: 0;
    overflow: visible;
    pointer-events: none;
  }
  .arrows path {
    fill: none;
    stroke: var(--accent);
    stroke-width: 1.5;
    opacity: 0.7;
  }
  .arrows path.hot {
    stroke-width: 2.5;
    opacity: 1;
  }
  .card {
    position: absolute;
    display: flex;
    gap: 8px;
    padding: 6px 8px;
    border: 1px solid var(--border);
    border-left: 3px solid var(--accent);
    border-radius: var(--r);
    background: var(--panel);
    box-sizing: border-box;
    transition: border-color 0.12s, background 0.12s;
  }
  .card.hot {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 7%, var(--panel));
  }
  .card.l {
    border-left: 1px solid var(--border);
    border-right: 3px solid var(--accent);
  }
  .card.loose {
    border-left-color: var(--border);
    border-right-color: var(--border);
  }
  .card.loose .num {
    background: var(--muted);
  }
  .card b {
    font-size: 13px;
  }
  .card p {
    margin: 2px 0 0;
    color: var(--muted);
    font-size: 12px;
  }
  .sub {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 3px 8px;
    margin: 5px 0 1px;
    font-size: 12px;
  }
  .sub dt {
    font-weight: 600;
    color: var(--text);
  }
  .sub dd {
    margin: 0;
    color: var(--muted);
  }
  h4 {
    margin: 16px 0 4px;
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--muted);
  }
  ul {
    margin: 0;
    padding-left: 18px;
  }
  li {
    margin: 2px 0;
  }
  .more {
    margin: 10px 0 0;
    font-size: 12px;
  }
</style>
