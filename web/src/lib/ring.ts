// A ring that spreads from an element and fades (a task done, a step done). It used to be an animated box-shadow,
// which repaints on every frame; here it is a separate ring that only grows (transform) and fades (opacity), so the
// compositor plays it at the screen's own rate.

/** Spread a ring `spread` px out from `el` over `ms`, in `color`, starting at `strength` (0..1) opacity. */
export function ring(el: HTMLElement, color: string, spread = 12, ms = 700, strength = 0.6) {
  const r = el.getBoundingClientRect();
  if (!r.width || !r.height) return;
  const o = document.createElement('span');
  o.setAttribute('aria-hidden', 'true');
  Object.assign(o.style, {
    position: 'fixed',
    left: `${r.left}px`,
    top: `${r.top}px`,
    width: `${r.width}px`,
    height: `${r.height}px`,
    borderRadius: getComputedStyle(el).borderRadius,
    boxShadow: `0 0 0 2px ${color}`,
    pointerEvents: 'none',
    zIndex: '60',
    willChange: 'transform, opacity',
  });
  document.body.append(o);
  const sx = (r.width + spread * 2) / r.width;
  const sy = (r.height + spread * 2) / r.height;
  const a = o.animate(
    [
      { transform: 'scale(1)', opacity: strength },
      { transform: `scale(${sx}, ${sy})`, opacity: 0 },
    ],
    { duration: ms, easing: 'ease-out' },
  );
  a.onfinish = a.oncancel = () => o.remove();
}
