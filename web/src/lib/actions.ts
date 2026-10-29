/** Calls `cb` when a pointer lands outside `node`, or Escape is pressed. */
export function dismiss(node: HTMLElement, cb: () => void) {
  const onDown = (e: PointerEvent) => {
    if (!node.contains(e.target as Node)) cb();
  };
  const onKey = (e: KeyboardEvent) => {
    if (e.key === 'Escape') cb();
  };
  // Defer so the click that opened the popover doesn't close it.
  const t = setTimeout(() => document.addEventListener('pointerdown', onDown, true));
  document.addEventListener('keydown', onKey);
  return {
    destroy() {
      clearTimeout(t);
      document.removeEventListener('pointerdown', onDown, true);
      document.removeEventListener('keydown', onKey);
    },
  };
}

export function autofocus(node: HTMLElement, enable = true) {
  if (enable) queueMicrotask(() => node.focus());
}
