/** Calls `cb` when a pointer lands outside `node`, or Escape is pressed. */
export function dismiss(node: HTMLElement, cb: () => void) {
  let handler = cb;
  const onDown = (e: PointerEvent) => {
    if (!node.contains(e.target as Node)) handler();
  };
  const onKey = (e: KeyboardEvent) => {
    if (e.key === 'Escape') handler();
  };
  // Defer so the click that opened the popover doesn't close it.
  const t = setTimeout(() => document.addEventListener('pointerdown', onDown, true));
  document.addEventListener('keydown', onKey);
  return {
    update(next: () => void) {
      handler = next;
    },
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
