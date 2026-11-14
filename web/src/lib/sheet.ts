// The menus under the message box (attach, permission, model, thinking, context, plan limits) do not float over it:
// the message box itself grows upward into the menu, with the app's spring overshoot, and folds back when it closes.
// The box's place is measured by Composer (`--cx-*` on .dock); the menus are laid over it there (composer-menus.css).

import type { TransitionConfig } from 'svelte/transition';

/** cubic-bezier(x1, y1, x2, y2) as a function of time, for curves whose y overshoots (the app's spring). */
export function bezier(x1: number, y1: number, x2: number, y2: number): (t: number) => number {
  const at = (a: number, b: number, t: number) => 3 * a * (1 - t) ** 2 * t + 3 * b * (1 - t) * t ** 2 + t ** 3;
  return (x: number) => {
    if (x <= 0) return 0;
    if (x >= 1) return 1;
    // solve x(t) = x by bisection (x is monotonic for these curves), then read y(t)
    let lo = 0;
    let hi = 1;
    for (let i = 0; i < 24; i++) {
      const mid = (lo + hi) / 2;
      if (at(x1, x2, mid) < x) lo = mid;
      else hi = mid;
    }
    return at(y1, y2, (lo + hi) / 2);
  };
}

/** The signature spring: a little past, then back (`cubic-bezier(0.3, 1.5, 0.5, 1)`). */
export const spring = bezier(0.3, 1.5, 0.5, 1);
const easeIn = bezier(0.4, 0, 1, 1);

const reduced = () => typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches;

/** Height of the message box the menu grows out of (set on .dock by Composer). */
function boxHeight(node: Element): number {
  const v = parseFloat(getComputedStyle(node).getPropertyValue('--cx-h'));
  return Number.isFinite(v) && v > 0 ? v : 48;
}

/** How a sheet moves: its height runs between the message box's and its own, bottom edge fixed. */
function move(node: Element, open: boolean): TransitionConfig {
  const total = (node as HTMLElement).offsetHeight;
  const hidden = Math.max(0, total - boxHeight(node));
  return {
    duration: reduced() ? 0 : open ? 560 : 170,
    easing: open ? spring : easeIn,
    // t: 0 → 1 takes the height from the box's to the menu's (the items sit at the bottom, so the top ones come into
    // view as it grows). Past 1, the overshoot, it really stands a little taller for a moment, then settles.
    css: (t) => `height: ${total - (1 - t) * hidden}px; overflow: hidden`,
  };
}

/** `in:grow`: the box stretches up into the menu, with the spring. */
export const grow = (node: Element): TransitionConfig => move(node, true);
/** `out:shrink`: the menu folds back into the box, quickly and without the spring. */
export const shrink = (node: Element): TransitionConfig => move(node, false);
