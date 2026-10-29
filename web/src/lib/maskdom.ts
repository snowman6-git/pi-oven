// Showcase "example" style: instead of blurring, the text of marked elements is overwritten with sample text. Only the
// text nodes' data changes (the original is kept aside and put back when the style is switched off), so Svelte's own
// bookkeeping stays valid, and the layout keeps roughly the same shape.

import type { Part } from './showcase.svelte';

const SAMPLE: Record<Part, string> = {
  paths: '~/example-project',
  user: 'user',
  titles: '예시 세션',
  files: 'example.ts',
  secrets: '••••••••',
  hosts: 'example.local',
  chat: '',
};

/** Words of an example text, so that a masked message still looks like a paragraph. */
const WORDS = ['예시', 'example', '샘플', 'showcase', '텍스트', 'sample'];

const masked = new Map<Text, { orig: string; shown: string }>();
let observer: MutationObserver | null = null;
let raf = 0;
let kinds: Part[] = [];

function wordy(s: string): string {
  let i = 0;
  return s.replace(/\S+/g, () => WORDS[i++ % WORDS.length]);
}

function textNodes(el: Element): Text[] {
  const out: Text[] = [];
  const w = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
  for (let n = w.nextNode(); n; n = w.nextNode()) {
    const t = n as Text;
    // Code editors and form controls keep their own text; those are covered by the blur fallback.
    if (t.parentElement?.closest('textarea,input,.cm-editor,style,script')) continue;
    out.push(t);
  }
  return out;
}

function kindOf(el: Element): Part | null {
  const tokens = (el.getAttribute('data-priv') ?? '').split(/\s+/);
  return kinds.find((k) => tokens.includes(k)) ?? null;
}

function pass() {
  raf = 0;
  for (const el of document.querySelectorAll('[data-priv]')) {
    const kind = kindOf(el);
    if (!kind) continue;
    let first = true;
    for (const t of textNodes(el)) {
      const prev = masked.get(t);
      if (prev && t.data === prev.shown) { first = first && !prev.shown; continue; }
      const orig = t.data;
      if (!orig.trim()) continue;
      const shown = kind === 'chat' ? wordy(orig) : first ? (kind === 'paths' && !orig.includes('/') ? 'example-project' : SAMPLE[kind]) : '';
      first = false;
      masked.set(t, { orig, shown });
      t.data = shown;
    }
  }
  for (const [t, m] of masked) if (!t.isConnected) masked.delete(t);
}

function schedule() {
  if (!raf) raf = requestAnimationFrame(pass);
}

/** Start (or re-aim) the masking for the given kinds; an empty list stops it. */
export function maskExample(parts: Part[]) {
  kinds = parts;
  if (!parts.length) return stop();
  if (!observer) {
    observer = new MutationObserver(schedule);
    observer.observe(document.body, { subtree: true, childList: true, characterData: true, attributes: true, attributeFilter: ['data-priv'] });
  }
  // A kind that was switched off: give its nodes their text back before the pass masks the rest.
  restore((el) => !kindOf(el));
  schedule();
}

function restore(only?: (el: Element) => boolean) {
  for (const [t, m] of masked) {
    const el = t.parentElement?.closest('[data-priv]');
    if (only && el && !only(el)) continue;
    if (t.data === m.shown) t.data = m.orig;
    masked.delete(t);
  }
}

function stop() {
  observer?.disconnect();
  observer = null;
  if (raf) cancelAnimationFrame(raf);
  raf = 0;
  restore();
}
