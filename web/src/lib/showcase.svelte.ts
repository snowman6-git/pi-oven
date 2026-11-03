// Showcase mode: hide what should not be on a screenshot or a shared screen. One switch for the mode, and one for each
// kind of thing, so a demo can show the file tree and still hide the folder names. The page is changed only in how it
// looks (blurred, or text replaced as it is drawn); nothing is stored differently.

import { pinRules } from './pinrules.svelte';
import { scrub, type ScrubOpts } from './scrub';

export type Style = 'blur' | 'example';

export type Part = 'paths' | 'user' | 'titles' | 'files' | 'secrets' | 'hosts' | 'chat';

export const PARTS: { id: Part; on: boolean }[] = [
  { id: 'paths', on: true },
  { id: 'user', on: true },
  { id: 'titles', on: true },
  { id: 'secrets', on: true },
  { id: 'hosts', on: true },
  { id: 'files', on: false },
  { id: 'chat', on: false },
];

const KEY = 'pi-oven.showcase';

function load(): { on: boolean; style: Style; parts: Record<Part, boolean> } {
  const parts = Object.fromEntries(PARTS.map((p) => [p.id, p.on])) as Record<Part, boolean>;
  try {
    const v = JSON.parse(localStorage.getItem(KEY) ?? 'null');
    if (v && typeof v === 'object') {
      for (const p of PARTS) if (typeof v.parts?.[p.id] === 'boolean') parts[p.id] = v.parts[p.id];
      // before v2 the stored style was only ever the old default (blur): those start on the new default, example text
      return { on: v.on === true, style: v.v === 2 && v.style === 'blur' ? 'blur' : 'example', parts };
    }
  } catch { /* private mode, or something else wrote here */ }
  return { on: false, style: 'example', parts };
}

class Showcase {
  on = $state(false);
  /** How a hidden thing is drawn: blurred, or overwritten with example text. */
  style = $state<Style>('example');
  parts = $state<Record<Part, boolean>>(Object.fromEntries(PARTS.map((p) => [p.id, p.on])) as Record<Part, boolean>);
  /** Folders and the home folder as this machine knows them (set by the app). */
  cwds = $state.raw<string[]>([]);
  home = $state('');

  constructor() {
    const v = load();
    this.on = v.on;
    this.style = v.style;
    this.parts = v.parts;
  }

  #save() {
    try { localStorage.setItem(KEY, JSON.stringify({ v: 2, on: this.on, style: this.style, parts: this.parts })); } catch { /* private mode */ }
  }

  setOn(on: boolean) {
    this.on = on;
    this.#save();
  }

  setStyle(style: Style) {
    this.style = style;
    this.#save();
  }

  set(part: Part, on: boolean) {
    this.parts[part] = on;
    this.#save();
  }

  active(part: Part) {
    return this.on && this.parts[part];
  }

  /** The kinds currently hidden, for the page's stylesheet (`[data-showcase~="files"]`). */
  get attr() {
    return this.on ? PARTS.filter((p) => this.parts[p.id]).map((p) => p.id).join(' ') : '';
  }

  /** The kinds whose text is overwritten (example style), for lib/maskdom. */
  get masked(): Part[] {
    return this.on && this.style === 'example' ? PARTS.filter((p) => this.parts[p.id]).map((p) => p.id) : [];
  }

  /** Text as it should be drawn: unchanged when the mode is off. */
  text = (s: string): string => {
    if (!this.on || !s) return s;
    const o: ScrubOpts = { cwds: this.cwds, home: this.home, paths: this.parts.paths, user: this.parts.user, secrets: this.parts.secrets, hosts: this.parts.hosts, rules: pinRules.compiled };
    return o.paths || o.user || o.secrets || o.hosts ? scrub(s, o) : s;
  };

  /** A tooltip that does not give away what the blurred text beside it hides. */
  tip = (part: Part, s: string | undefined): string | undefined => (this.active(part) ? undefined : s);
}

export const showcase = new Showcase();
