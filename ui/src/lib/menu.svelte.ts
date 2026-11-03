// The right-click menu: one for the whole app (ContextMenu.svelte shows it).
// A component calls openMenu(e, items) from its contextmenu handler.

export interface MenuItem {
  label: string;
  run?: () => void;
  /** shown on the right: a key, or a value */
  hint?: string;
  disabled?: boolean;
  /** a tick in front (a setting that is on) */
  checked?: boolean;
  danger?: boolean;
}

/** a line, or a small heading over the items after it */
export type MenuEntry = MenuItem | { sep: true } | { head: string };

export const menu = $state<{ x: number; y: number; items: MenuEntry[] } | { x: number; y: number; items: null }>({
  x: 0,
  y: 0,
  items: null,
});

export function openMenu(e: MouseEvent, items: MenuEntry[]) {
  e.preventDefault();
  e.stopPropagation();
  // no line or heading at the ends, no two lines in a row
  const clean: MenuEntry[] = [];
  for (const it of items) {
    if ('sep' in it && (!clean.length || 'sep' in clean[clean.length - 1])) continue;
    clean.push(it);
  }
  while (clean.length && 'sep' in clean[clean.length - 1]) clean.pop();
  if (!clean.length) return;
  menu.x = e.clientX;
  menu.y = e.clientY;
  menu.items = clean;
}

export function closeMenu() {
  menu.items = null;
}
