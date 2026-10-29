// Streaming text is re-rendered from markdown on every frame, so a CSS animation on the
// elements would restart each time. Instead every word remembers when it first appeared and is
// wrapped in a span whose animation-delay is negative by its age: the effect carries on from where
// it was, and words that finished are left as plain text.

export const FX_MS = 520;

export const escapeHtml = (t: string) => t.replace(/[&<>]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;' })[c]!);

export class Reveal {
  #births: number[] = [];
  started = false;

  /**
   * Plain text (thinking). Words older than the effect are one untouched string; only the tail
   * that is still animating becomes spans, so the cost does not grow with the length of the text.
   */
  applyText(text: string): string {
    const now = performance.now();
    this.#births.length = Math.min(this.#births.length, text.length);
    while (this.#births.length < text.length) this.#births.push(now);

    let cut = text.length;
    while (cut > 0 && now - this.#births[cut - 1] < FX_MS) cut--;
    while (cut > 0 && !/\s/.test(text[cut - 1])) cut--; // start the tail on a word boundary

    let out = escapeHtml(text.slice(0, cut));
    let offset = cut;
    for (const part of text.slice(cut).match(/\s+|\S+/g) ?? []) {
      const age = now - this.#births[offset];
      out += /\S/.test(part) && age < FX_MS
        ? `<span class="w" style="animation-delay:-${Math.round(age)}ms">${escapeHtml(part)}</span>`
        : escapeHtml(part);
      offset += part.length;
    }
    return out;
  }

  apply(html: string): string {
    const now = performance.now();
    const tpl = document.createElement('template');
    tpl.innerHTML = html;

    const nodes: Text[] = [];
    const walker = document.createTreeWalker(tpl.content, NodeFilter.SHOW_TEXT);
    while (walker.nextNode()) nodes.push(walker.currentNode as Text);

    let offset = 0;
    for (const node of nodes) {
      const text = node.data;
      while (this.#births.length < offset + text.length) this.#births.push(now);
      if (node.parentElement?.closest('pre, button')) { offset += text.length; continue; }

      const frag = document.createDocumentFragment();
      let fresh = false;
      for (const part of text.match(/\s+|\S+/g) ?? []) {
        const age = now - this.#births[offset];
        if (/\S/.test(part) && age < FX_MS) {
          const span = document.createElement('span');
          span.className = 'w';
          span.style.animationDelay = `-${Math.round(age)}ms`;
          span.textContent = part;
          frag.append(span);
          fresh = true;
        } else frag.append(part);
        offset += part.length;
      }
      if (fresh || frag.childNodes.length) node.replaceWith(frag);
    }
    return tpl.innerHTML;
  }
}
