// What a person has pinned in a folder: addresses, tokens, notes, and messages from the conversation. Pins belong to
// the folder, not to one session (the server you are building stays the point of the project), and live in this
// browser only.

export interface Pin {
  id: string;
  kind: 'url' | 'secret' | 'note' | 'message';
  label: string;
  value: string;
  /** For a message: who said it, and which one (so pinning twice is unpinning). */
  role?: 'user' | 'assistant';
  mkey?: string;
}

const MAX_TEXT = 20_000;

function hash(s: string): string {
  let h = 5381;
  for (let i = 0; i < s.length; i++) h = ((h << 5) + h + s.charCodeAt(i)) | 0;
  return (h >>> 0).toString(36);
}

/** The same message always gets the same key, whatever the order things loaded in. */
export const messageKey = (role: string, text: string, ts?: number) => `${role}:${ts ?? 0}:${hash(text)}`;

export const firstLine = (text: string, max = 60) => {
  const line = text.trim().split('\n')[0].replace(/\s+/g, ' ');
  return line.length > max ? line.slice(0, max) + '…' : line || '(빈 메시지)';
};

export class Pins {
  list = $state<Pin[]>([]);
  readonly #key: string;

  constructor(cwd: string) {
    this.#key = `pi-oven.pins:${cwd}`;
    try {
      const v = JSON.parse(localStorage.getItem(this.#key) ?? '[]');
      if (Array.isArray(v)) this.list = v.filter((p) => p && typeof p.id === 'string' && typeof p.value === 'string');
    } catch { /* private mode, or something else wrote here */ }
  }

  #save() {
    try { localStorage.setItem(this.#key, JSON.stringify(this.list)); } catch { /* private mode, or full */ }
  }

  hasValue(value: string) {
    return this.list.some((p) => p.kind !== 'message' && p.value === value);
  }

  hasMessage(mkey: string) {
    return this.list.some((p) => p.mkey === mkey);
  }

  add(kind: Exclude<Pin['kind'], 'message'>, label: string, value: string) {
    if (!value || this.hasValue(value)) return;
    this.list = [{ id: Math.random().toString(36).slice(2), kind, label, value }, ...this.list];
    this.#save();
  }

  /** Pins a message, or takes it off again if it was pinned. Returns whether it is pinned now. */
  toggleMessage(role: 'user' | 'assistant', text: string, ts?: number): boolean {
    const mkey = messageKey(role, text, ts);
    const had = this.list.find((p) => p.mkey === mkey);
    if (had) {
      this.remove(had.id);
      return false;
    }
    const value = text.length > MAX_TEXT ? text.slice(0, MAX_TEXT) + '\n…(이하 생략)' : text;
    this.list = [{ id: Math.random().toString(36).slice(2), kind: 'message', label: firstLine(text), value, role, mkey }, ...this.list];
    this.#save();
    return true;
  }

  remove(id: string) {
    this.list = this.list.filter((p) => p.id !== id);
    this.#save();
  }
}

const shared = new Map<string, Pins>();
/** One list per folder, however many sessions of it are open. */
export const pinsFor = (cwd: string) => shared.get(cwd) ?? (shared.set(cwd, new Pins(cwd)), shared.get(cwd)!);
