// The person's own rules for what the Pin tab should pick up (regular expressions), kept in this browser.

import { compileRule, ruleProblem, type CompiledRule } from './points';

export interface PinRule { id: string; name: string; pattern: string; on: boolean }

const KEY = 'pi-oven.pinrules';
const MAX = 20;

function load(): PinRule[] {
  try {
    const v = JSON.parse(localStorage.getItem(KEY) ?? '[]');
    if (!Array.isArray(v)) return [];
    return v
      .filter((r) => r && typeof r.pattern === 'string' && typeof r.name === 'string')
      .slice(0, MAX)
      .map((r) => ({ id: String(r.id ?? crypto.randomUUID()), name: r.name.slice(0, 40), pattern: r.pattern, on: r.on !== false }));
  } catch {
    return [];
  }
}

class PinRules {
  list = $state<PinRule[]>(load());

  #save() {
    try { localStorage.setItem(KEY, JSON.stringify(this.list)); } catch { /* private mode */ }
  }

  /** Switched-on rules that compile; a broken one is skipped, never thrown. */
  compiled = $derived.by<CompiledRule[]>(() =>
    this.list.filter((r) => r.on).flatMap((r) => { const c = compileRule(r.name, r.pattern); return c ? [c] : []; }));

  /** Returns why it was refused, or null once added. */
  add(name: string, pattern: string): string | null {
    const problem = ruleProblem(pattern) ?? (this.list.length >= MAX ? `규칙은 ${MAX}개까지 만들 수 있습니다.` : null);
    if (problem) return problem;
    this.list.push({ id: crypto.randomUUID(), name: name.trim().slice(0, 40) || '규칙', pattern, on: true });
    this.#save();
    return null;
  }

  toggle(id: string, on: boolean) {
    const r = this.list.find((x) => x.id === id);
    if (r) { r.on = on; this.#save(); }
  }

  remove(id: string) {
    this.list = this.list.filter((r) => r.id !== id);
    this.#save();
  }
}

export const pinRules = new PinRules();
