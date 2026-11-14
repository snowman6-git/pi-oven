// The menus under the message box (attach, permission, model, thinking, context, plan limits) do not float over it:
// the message box itself grows upward into the menu, with the app's spring overshoot, and folds back when it closes.
// The box's place is measured by Composer (`--cx-*` on .dock); the menus are laid over it there (composer-menus.css).
//
// The motion is run with the Web Animations API, not Svelte's css transitions: Svelte samples a curve 60 times a
// second, which shows as a slightly stepped overshoot on a fast phone screen. Here the curve is sampled densely and
// the browser plays it at the display's own rate. Svelte only waits for the duration.

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

/** However tall the thing, the spring goes at most this far past and back (a tall list would otherwise bounce far). */
const MAX_PAST = 12;
/** A height on the way from `from` to `to`: the spring's overshoot past `to` is kept to MAX_PAST px. */
export const toward = (from: number, to: number, t: number) =>
  t <= 1 ? from + (to - from) * t : to + Math.min(MAX_PAST, (to - from) * (t - 1));

/** Samples per second of the curve handed to the browser (it plays them at the screen's rate, 120 Hz or more). */
const SAMPLES = 240;

const reduced = () => typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches;

/** Height of the message box the menu grows out of (set on .dock by Composer). */
function boxHeight(node: Element): number {
  const v = parseFloat(getComputedStyle(node).getPropertyValue('--cx-h'));
  return Number.isFinite(v) && v > 0 ? v : 48;
}

/**
 * Animate `node`'s height from `from` to `to` px along `ease`, sampled densely. Returns what Svelte needs to wait for.
 * `max-height` is lifted while it moves, or a list at its cap could not overshoot.
 */
function heightTo(node: Element, from: number, to: number, ms: number, ease: (t: number) => number): TransitionConfig {
  if (reduced() || ms <= 0 || typeof (node as HTMLElement).animate !== 'function') return { duration: 0 };
  const n = Math.max(2, Math.ceil((ms / 1000) * SAMPLES));
  const frames: Keyframe[] = [];
  for (let i = 0; i <= n; i++) {
    const p = i / n;
    frames.push({ offset: p, height: `${toward(from, to, ease(p))}px`, maxHeight: 'none', overflow: 'hidden' });
  }
  // closing keeps its last frame until Svelte removes the node; opening ends on the node's own height
  (node as HTMLElement).animate(frames, { duration: ms, easing: 'linear', fill: to < from ? 'forwards' : 'none' });
  return { duration: ms };
}

/** `in:grow`: the box stretches up into the menu, with the spring. */
export function grow(node: Element): TransitionConfig {
  const total = (node as HTMLElement).offsetHeight;
  return heightTo(node, Math.min(total, boxHeight(node)), total, 280, spring);
}

/** `out:shrink`: the menu folds back into the box, quickly and without the spring. */
export function shrink(node: Element): TransitionConfig {
  const total = (node as HTMLElement).offsetHeight;
  return heightTo(node, total, Math.min(total, boxHeight(node)), 85, easeIn);
}

/** `in:rise`: something added inside the message box (attached images, the file chip, the command list) opens
    the box upward from nothing, with the spring, instead of appearing at once. */
export function rise(node: Element): TransitionConfig {
  return heightTo(node, 0, (node as HTMLElement).offsetHeight, 260, spring);
}

/** `out:sink`: and closes again, quickly. */
export function sink(node: Element): TransitionConfig {
  return heightTo(node, (node as HTMLElement).offsetHeight, 0, 75, easeIn);
}
