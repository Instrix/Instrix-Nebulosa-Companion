<script lang="ts">
  // A section's "?": a live snapshot of the section (a copy of it as it is now,
  // values included, not clickable) with each option boxed and numbered, and
  // the numbered list of what they do beside it. Pointing at an item (or a box)
  // lights the pair up and draws one leader between them; nothing else is drawn,
  // so the picture stays clean however many options there are. A copy rather
  // than a picture: it always matches the app, the theme and the device.
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

  type Box = { x: number; y: number; w: number; h: number };
  const LISTS = new Set(['DL', 'UL', 'OL', 'TABLE', 'TBODY']);
  const LIST_W = 400;
  const GUTTER = 56;

  /** the snapshot's frame (boxes are placed in it) and the whole body (the leader is drawn over it) */
  let frame = $state<HTMLDivElement>();
  let body = $state<HTMLDivElement>();
  let snapW = $state(320);
  let snapH = $state(0);
  let boxes = $state<(Box | null)[]>([]);
  /** the items in the order their options appear (the ones not showing last) */
  let order = $state<number[]>([]);
  let hot = $state<number | null>(null);
  let ready = $state(false);
  let itemEls: HTMLElement[] = $state([]);
  let snapRoot: HTMLElement | null = null;
  /** the leader for the item pointed at */
  let leader = $state<{ d: string; w: number; h: number } | null>(null);

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
    // a section in its own window (Section.svelte): the copy keeps its width and
    // layout, not its place on the screen or its fixed height
    if (copy.classList.contains('fwin')) {
      copy.style.cssText = `position:relative;left:auto;top:auto;z-index:auto;width:${sec.getBoundingClientRect().width}px;height:auto;resize:none;box-shadow:none;`;
      copy.querySelectorAll<HTMLElement>('.wbody').forEach((b) => (b.style.overflow = 'visible'));
    }
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
    if (!frame || !snapRoot) return;
    const f = frame.getBoundingClientRect();
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
      return { x: x0 - f.left - 3, y: y0 - f.top - 3, w: x1 - x0 + 6, h: y1 - y0 + 6 };
    });
    order = help.items
      .map((_, k) => k)
      .sort((a, b) => (boxes[a]?.y ?? 1e9) - (boxes[b]?.y ?? 1e9) || (boxes[a]?.x ?? 0) - (boxes[b]?.x ?? 0));
    snapH = snapRoot.getBoundingClientRect().bottom - f.top;
    ready = true;
    drawLeader();
  }

  /** the number shown for item k: its place in the list */
  const num = (k: number) => order.indexOf(k) + 1;

  /**
   * The one leader: from the item, across the gutter, into its box's side; for
   * a control mid-row (others to its right), along the gap above the row and
   * down into its top, so it doesn't cross the neighbours.
   */
  function drawLeader() {
    const k = hot;
    const b = k == null ? null : boxes[k];
    const item = k == null ? null : itemEls[k];
    if (!b || !item || !frame || !body) {
      leader = null;
      return;
    }
    const o = body.getBoundingClientRect();
    const f = frame.getBoundingClientRect();
    const r = item.getBoundingClientRect();
    const fx = f.left - o.left;
    const fy = f.top - o.top;
    const x1 = r.left - o.left - 2;
    const y1 = r.top - o.top + Math.min(16, r.height / 2);
    const lane = fx + snapW + GUTTER / 2;
    const bx = fx + b.x;
    const by = fy + b.y;
    const midRow = b.w < snapW * 0.6 && snapW - (b.x + b.w) > 16;
    let pts: [number, number][];
    if (midRow) {
      const gy = by - 5;
      const cx = bx + b.w / 2;
      pts = [[x1, y1], [lane, y1], [lane, gy], [cx, gy], [cx, by - 1]];
    } else {
      const y2 = by + b.h / 2;
      pts = Math.abs(y1 - y2) < 2 ? [[x1, y2], [bx + b.w + 2, y2]] : [[x1, y1], [lane, y1], [lane, y2], [bx + b.w + 2, y2]];
    }
    leader = { d: poly(pts), w: o.width, h: o.height };
  }

  $effect(() => {
    void hot;
    drawLeader();
  });

  /** a polyline with its corners rounded */
  function poly(pts: [number, number][], rad = 8): string {
    let d = `M ${pts[0][0]} ${pts[0][1]}`;
    for (let i = 1; i < pts.length - 1; i++) {
      const [px, py] = pts[i - 1];
      const [cx, cy] = pts[i];
      const [nx, ny] = pts[i + 1];
      const lin = Math.hypot(cx - px, cy - py);
      const lout = Math.hypot(nx - cx, ny - cy);
      const r = Math.min(rad, lin / 2, lout / 2);
      if (!(r > 0.5)) {
        d += ` L ${cx} ${cy}`;
        continue;
      }
      d += ` L ${cx - ((cx - px) / lin) * r} ${cy - ((cy - py) / lin) * r}`;
      d += ` Q ${cx} ${cy} ${cx + ((nx - cx) / lout) * r} ${cy + ((ny - cy) / lout) * r}`;
    }
    const [lx, ly] = pts[pts.length - 1];
    return d + ` L ${lx} ${ly}`;
  }

  function snapshot(host: HTMLElement) {
    snapW = Math.round(sec.getBoundingClientRect().width) || 320;
    // let the panel show the options its current mode hides, copy it, then back
    dispatchEvent(new CustomEvent('nebulosa:help-preview', { detail: id }));
    requestAnimationFrame(() => {
      buildSnapshot(host);
      dispatchEvent(new CustomEvent('nebulosa:help-preview', { detail: null }));
      requestAnimationFrame(measure);
    });
    const ro = new ResizeObserver(() => measure());
    ro.observe(host);
    return { destroy: () => ro.disconnect() };
  }

  /** point at item k (from its box: bring its description into view) */
  function point(k: number | null, reveal = false) {
    hot = k;
    if (reveal && k != null) itemEls[k]?.scrollIntoView({ block: 'nearest' });
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
      return;
    }
    // ↑ / ↓: through the items in order
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      e.stopPropagation();
      const i = hot == null ? -1 : order.indexOf(hot);
      const n = order.length;
      const j = e.key === 'ArrowDown' ? (i + 1) % n : (i - 1 + n) % n;
      point(order[j], true);
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
    style="width: min({snapW + GUTTER + LIST_W + 48}px, 96vw)"
    onclick={(e) => e.stopPropagation()}
  >
    <header>
      {#if icon}<Icon name={icon} />{/if}
      <h3>{title}</h3>
      <span class="muted small">— what's here</span>
      <button class="ghost small hclose" onclick={onclose} aria-label="Close">✕</button>
    </header>
    <p class="intro">{help.intro}</p>

    <div class="scroll" onscroll={drawLeader}>
      <div class="hbody" class:ready class:pointing={hot != null} bind:this={body} style="grid-template-columns: {snapW}px {GUTTER}px minmax(240px, {LIST_W}px)">
        <div class="snapcol">
          <div class="frame" bind:this={frame} style="width: {snapW}px; height: {snapH}px">
            <div class="snap" inert use:snapshot></div>
            {#each boxes as b, k (k)}
              {#if b}
                <!-- svelte-ignore a11y_no_static_element_interactions -->
                <div
                  class="box"
                  class:hot={hot === k}
                  style="left: {b.x}px; top: {b.y}px; width: {b.w}px; height: {b.h}px"
                  onmouseenter={() => point(k, true)}
                  onmouseleave={() => point(null)}
                >
                  <span class="num">{num(k)}</span>
                </div>
              {/if}
            {/each}
          </div>
        </div>

        <ol class="list" style="grid-column: 3">
          {#each order as k (k)}
            {@const it = help.items[k]}
            <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
            <li
              class:hot={hot === k}
              class:loose={!boxes[k]}
              bind:this={itemEls[k]}
              tabindex="0"
              onmouseenter={() => point(k)}
              onmouseleave={() => point(null)}
              onfocus={() => point(k)}
              title={boxes[k] ? '' : 'Not showing with the current settings'}
            >
              <span class="num">{boxes[k] ? num(k) : '–'}</span>
              <div>
                <b>{it.name}</b>
                <p>{it.text}</p>
                {#if it.list}
                  <dl class="sub">
                    {#each it.list as [t, v] (t)}<dt>{t}</dt><dd>{v}</dd>{/each}
                  </dl>
                {/if}
              </div>
            </li>
          {/each}
        </ol>

        {#if leader}
          <svg class="leader" width={leader.w} height={leader.h} aria-hidden="true">
            <defs>
              <marker id="sh-head" viewBox="0 0 10 10" refX="8" refY="5" markerWidth="8" markerHeight="8" orient="auto-start-reverse">
                <path d="M 0 1 L 9 5 L 0 9 z" fill="context-stroke" />
              </marker>
            </defs>
            <path d={leader.d} marker-end="url(#sh-head)" />
            <circle cx={leader.d.split(' ')[1]} cy={leader.d.split(' ')[2]} r="3" />
          </svg>
        {/if}
      </div>

      {#if help.tips?.length}
        <h4>How to use</h4>
        <ul class="tips">
          {#each help.tips as t (t)}<li>{t}</li>{/each}
        </ul>
      {/if}
      <p class="more muted">Point at an item, or use ↑ ↓, to see where it is. Items marked – aren't showing with the current settings. More in Help (? or F1) and the user manual.</p>
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
    overflow: auto;
    min-height: 0;
    padding: 8px 4px 0 8px;
  }
  .hbody {
    position: relative;
    display: grid;
    align-items: start;
    opacity: 0;
    transition: opacity 0.15s;
  }
  .hbody.ready {
    opacity: 1;
  }
  /* the snapshot stays in view while a long list scrolls */
  .snapcol {
    position: sticky;
    top: 0;
  }
  .frame {
    position: relative;
  }
  .snap {
    /* the section's own width: a frame outside it, not padding inside */
    outline: 1px dashed var(--border);
    outline-offset: 7px;
    border-radius: var(--r);
    background: var(--bg);
    box-shadow: 0 0 0 7px var(--bg);
    pointer-events: none;
    user-select: none;
  }
  .box {
    position: absolute;
    border: 1.5px solid color-mix(in srgb, var(--accent) 75%, transparent);
    border-radius: 5px;
    background: color-mix(in srgb, var(--accent) 6%, transparent);
    cursor: help;
    transition: opacity 0.12s, background 0.12s, box-shadow 0.12s;
  }
  .pointing .box:not(.hot) {
    opacity: 0.3;
  }
  .box.hot {
    border-color: var(--accent);
    border-width: 2px;
    background: color-mix(in srgb, var(--accent) 16%, transparent);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 30%, transparent);
  }
  .num {
    display: inline-grid;
    place-items: center;
    flex: none;
    min-width: 18px;
    height: 18px;
    padding: 0 4px;
    border-radius: 9px;
    background: var(--accent);
    color: #fff;
    font-size: 11px;
    font-weight: 700;
    line-height: 1;
  }
  /* on the box: its top-right corner, clear of the labels (they start at the left) */
  .box .num {
    position: absolute;
    right: -8px;
    top: -9px;
    box-shadow: 0 0 0 2px var(--panel);
  }
  .list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .list li {
    display: flex;
    gap: 10px;
    padding: 7px 10px;
    border-radius: var(--r);
    border-left: 3px solid transparent;
    cursor: default;
    outline: none;
    transition: background 0.12s, border-color 0.12s;
  }
  .list li:hover,
  .list li.hot {
    background: color-mix(in srgb, var(--accent) 10%, var(--panel));
    border-left-color: var(--accent);
  }
  .list li.loose .num {
    background: var(--border);
    color: var(--muted);
  }
  .list li.loose b {
    color: var(--muted);
  }
  .list b {
    font-size: 13px;
  }
  .list p {
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
  }
  .sub dd {
    margin: 0;
    color: var(--muted);
  }
  .leader {
    position: absolute;
    left: 0;
    top: 0;
    overflow: visible;
    pointer-events: none;
  }
  .leader path {
    fill: none;
    stroke: var(--accent);
    stroke-width: 2;
    stroke-linejoin: round;
  }
  .leader circle {
    fill: var(--accent);
  }
  h4 {
    margin: 16px 0 4px;
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--muted);
  }
  .tips {
    margin: 0;
    padding-left: 18px;
  }
  .tips li {
    margin: 2px 0;
  }
  .more {
    margin: 10px 0 0;
    font-size: 12px;
  }
</style>
