// Bringing a part of the conversation into view from elsewhere (a pin, a chip). The conversation is told, so that it
// stops following the answer: the move is the reader's wish, not the layout's.

export const SHOW = 'oven-show';

export function showIn(el: Element | null | undefined) {
  if (!el) return;
  el.dispatchEvent(new CustomEvent(SHOW, { bubbles: true }));
  el.scrollIntoView({ block: 'center', behavior: 'smooth' });
}
