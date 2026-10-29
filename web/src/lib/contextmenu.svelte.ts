import type { IconName } from './icons';

export type MenuItem =
  | { label: string; run: () => void; icon?: IconName; danger?: boolean; disabled?: boolean }
  | 'sep';

class MenuState {
  items = $state<MenuItem[]>([]);
  x = $state(0);
  y = $state(0);
  open = $state(false);
  /** Touch devices get a bottom sheet instead of a floating menu. */
  sheet = $state(false);
  shownAt = 0;

  show(x: number, y: number, items: MenuItem[], sheet = false) {
    const real = items.filter((it, i) => !(it === 'sep' && (i === 0 || items[i - 1] === 'sep' || i === items.length - 1)));
    if (!real.some((i) => i !== 'sep')) return;
    this.items = real;
    this.x = x;
    this.y = y;
    this.sheet = sheet;
    this.open = true;
    this.shownAt = performance.now();
  }

  hide() {
    this.open = false;
  }
}

export const menu = new MenuState();

type Build = (e: { clientX: number; clientY: number; target: EventTarget | null }) => MenuItem[] | null;

const LONG_PRESS_MS = 520;
const MOVE_TOLERANCE = 9;

/**
 * Right-click (and long-press on touch) opens `build(...)`'s items instead of the browser menu.
 * Returning null keeps the native menu, which is what we want inside text fields.
 */
export function contextMenu(node: HTMLElement, build: Build) {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let sx = 0, sy = 0;

  const open = (e: { clientX: number; clientY: number; target: EventTarget | null }, touch: boolean) => {
    const items = build(e);
    if (!items?.length) return false;
    menu.show(e.clientX, e.clientY, items, touch || matchMedia('(pointer: coarse)').matches);
    return true;
  };

  const oncontextmenu = (e: MouseEvent) => {
    // Android fires this itself after a long press; don't open twice.
    if (performance.now() - menu.shownAt < 800 && menu.open) { e.preventDefault(); return; }
    if (open(e, false)) e.preventDefault();
  };

  const cancel = () => { clearTimeout(timer); timer = undefined; };

  const onpointerdown = (e: PointerEvent) => {
    if (e.pointerType !== 'touch') return;
    cancel();
    sx = e.clientX; sy = e.clientY;
    const target = e.target;
    timer = setTimeout(() => {
      if (open({ clientX: sx, clientY: sy, target }, true)) {
        // The finger is still down: swallow only the click that its own release produces, nothing later,
        // so a quick tap on a menu item right after the sheet opens still works.
        const swallow = (ev: Event) => { ev.preventDefault(); ev.stopPropagation(); };
        const disarm = () => removeEventListener('click', swallow, true);
        addEventListener('click', swallow, { capture: true });
        const released = () => setTimeout(disarm, 140);
        addEventListener('pointerup', released, { once: true, capture: true });
        addEventListener('pointercancel', released, { once: true, capture: true });
        setTimeout(disarm, 3000);   // never leave it armed if the release was lost
        navigator.vibrate?.(8);
      }
    }, LONG_PRESS_MS);
  };
  const onpointermove = (e: PointerEvent) => {
    if (timer && Math.hypot(e.clientX - sx, e.clientY - sy) > MOVE_TOLERANCE) cancel();
  };

  node.addEventListener('contextmenu', oncontextmenu);
  node.addEventListener('pointerdown', onpointerdown);
  node.addEventListener('pointermove', onpointermove);
  node.addEventListener('pointerup', cancel);
  node.addEventListener('pointercancel', cancel);
  return {
    update(next: Build) { build = next; },
    destroy() {
      cancel();
      node.removeEventListener('contextmenu', oncontextmenu);
      node.removeEventListener('pointerdown', onpointerdown);
      node.removeEventListener('pointermove', onpointermove);
      node.removeEventListener('pointerup', cancel);
      node.removeEventListener('pointercancel', cancel);
    },
  };
}
