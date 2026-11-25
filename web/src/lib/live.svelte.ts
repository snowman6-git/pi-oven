// What is happening to the files of one conversation's folder: what the model is composing (read from its tool calls
// before they run), what pi's tools saved, and what the server's folder watch saw change (bash, subagents, anything
// else). The file tree marks them and the live pane under it follows the newest.

import { sessionChanges, type FileChange } from './changes';
import { draftsOf, inFolder, type Draft } from './composing';
import type { Item, ToolBlock } from './transcript';

export interface LiveHost {
  cwd: string;
  items(): Item[];
  running(): boolean;
  /** A permission question for a tool is on screen. */
  asking(tool: string): boolean;
  /** Files on the disk changed: what is open in the editor should look again. */
  reread(paths: string[] | null): void;
  /** The person saved this file here a moment ago. */
  savedHere(path: string): boolean;
}

export type Phase = 'composing' | 'waiting' | 'ask' | 'running';
export interface LiveDraft { key: string; id: string; draft: Draft; phase: Phase }
export type Mark = 'new' | 'mod';
export interface WatchedFile { p: string; k: 'new' | 'mod' | 'gone'; dir?: boolean }

/** More than this at once (a checkout, a generator) is marked, but the view does not run through it. */
const MANY = 5;
const FAILED_MS = 2600;
const FRESH_MS = 900;

export class Live {
  /** Not looked at since it changed. By path from the folder. */
  marks = $state<Record<string, Mark>>({});
  /** Made during this session (stays known after the mark is gone). */
  made = $state<Record<string, true>>({});
  /** Marked just now (the row springs once). */
  fresh = $state<Record<string, true>>({});
  /** Changed last by something other than pi's own tools: the disk says what it holds. Bumped with each change. */
  disk = $state<Record<string, { k: Mark; n: number; at: number }>>({});
  /** When each path last changed (a count). */
  order = $state<Record<string, number>>({});
  /** Any change on the disk (also ones that show nowhere yet). */
  n = $state(0);
  follow = $state(true);
  picked = $state<string | null>(null);
  /** Where following goes when nothing is being composed. */
  aim = $state<string | null>(null);
  failed = $state<{ key: string; draft: Draft; text: string; declined: boolean } | null>(null);

  /** The explorer reads these folders again (null: all it shows). */
  onDirs: ((dirs: string[] | null) => void) | null = null;

  #count = 0;
  #toolAt = new Map<string, number>();
  #failTimer: ReturnType<typeof setTimeout> | undefined;

  constructor(private host: LiveHost) {}

  key(path: string) {
    return inFolder(path, this.host.cwd) ?? path;
  }

  /** What pi's tools changed this session, by path from the folder. */
  changes = $derived.by<Map<string, FileChange>>(() => new Map(sessionChanges(this.host.items(), (p) => this.key(p)).map((f) => [f.path, f])));

  drafts = $derived.by<LiveDraft[]>(() => {
    if (!this.host.running()) return [];
    const items = this.host.items();
    let blocks: ToolBlock[] = [];
    for (let i = items.length - 1; i >= 0 && i > items.length - 12; i--) {
      const it = items[i];
      if (it.kind === 'user') break;
      if (it.kind !== 'assistant') continue;
      blocks = it.blocks.filter((b): b is ToolBlock => !!b && b.type === 'tool' && (b.status === 'pending' || b.status === 'running'));
      break;
    }
    const out = new Map<string, LiveDraft>();
    for (const b of blocks) {
      // a path still being written names some other file
      if (b.composing && b.name !== 'bash' && (!b.partial || /^(path|file_path)$/.test(Object.keys(b.partial).at(-1) ?? 'path'))) continue;
      for (const draft of draftsOf(b.name, b.partial ?? b.args)) {
        const key = this.key(draft.path);
        const phase: Phase = b.composing ? 'composing' : b.status === 'pending' ? 'waiting' : this.host.asking(b.name) ? 'ask' : 'running';
        // a shell command goes on after writing the file: what is on the disk is then shown from the disk
        if (draft.shell && phase === 'running' && (this.disk[key]?.at ?? 0) > (b.startedAt ?? 0)) continue;
        out.delete(key);
        out.set(key, { key, id: b.id, draft, phase });
      }
    }
    return [...out.values()];
  });

  /** The one in front: what is being written now, else the first that has not been saved. */
  get draft(): LiveDraft | null {
    const d = this.drafts;
    return d.findLast((x) => x.phase === 'composing') ?? d.find((x) => x.phase === 'ask') ?? d[0] ?? null;
  }

  draftOf(key: string): LiveDraft | undefined {
    return this.drafts.find((d) => d.key === key);
  }

  /** Changed files, the latest first. */
  history = $derived.by<string[]>(() => {
    const all = new Set([...Object.keys(this.order), ...this.changes.keys()]);
    const seq = (p: string) => (this.order[p] ?? 0) * 1e6 + (this.changes.get(p)?.lastSeq ?? 0);
    return [...all].sort((a, b) => seq(b) - seq(a));
  });

  /** Where the newest change is. */
  get target(): string | null {
    return this.draft?.key ?? this.failed?.key ?? (this.aim && this.history.includes(this.aim) ? this.aim : null) ?? this.history[0] ?? null;
  }

  get view(): string | null {
    return this.follow ? this.target : this.picked ?? this.target;
  }

  kind(key: string): Mark {
    return this.marks[key] ?? (this.made[key] ? 'new' : 'mod');
  }

  get active() {
    return this.history.length > 0 || this.drafts.length > 0 || !!this.failed;
  }

  pick(key: string) {
    this.picked = key;
    this.follow = false;
    this.seen(key);
  }

  setFollow(on: boolean) {
    if (!on) this.picked = this.view;
    this.follow = on;
  }

  /** The person has looked at it. */
  seen(key: string) {
    if (!this.marks[key]) return;
    const { [key]: _, ...rest } = this.marks;
    this.marks = rest;
  }

  clearMarks() {
    this.marks = {};
  }

  #mark(key: string, k: Mark) {
    if (k === 'new') this.made = { ...this.made, [key]: true };
    // a file that is new stays new however often it changes, until it is looked at
    this.marks = { ...this.marks, [key]: this.marks[key] === 'new' ? 'new' : k };
    this.fresh = { ...this.fresh, [key]: true };
    setTimeout(() => { const { [key]: _, ...rest } = this.fresh; this.fresh = rest; }, FRESH_MS);
  }

  // ── what pi says ───────────────────────────────────────────────────────────

  ended(b: ToolBlock, declined: boolean) {
    if (b.name !== 'edit' && b.name !== 'write') return;
    const draft = draftsOf(b.name, b.args)[0];
    if (!draft) return;
    const key = this.key(draft.path);
    if (b.status === 'error') {
      clearTimeout(this.#failTimer);
      this.failed = { key, draft, text: b.output.trim().split('\n')[0].slice(0, 200), declined };
      this.#failTimer = setTimeout(() => (this.failed = null), FAILED_MS);
      return;
    }
    this.failed = null;
    this.#toolAt.set(key, Date.now());
    const { [key]: _, ...rest } = this.disk;
    this.disk = rest;
    this.order = { ...this.order, [key]: ++this.#count };
    this.aim = key;
    this.onDirs?.([key.includes('/') ? key.slice(0, key.lastIndexOf('/')) : '']);
    // (the watch says "new" when it was: it may have come before this, or not at all)
    if (!this.marks[key]) this.#mark(key, 'mod');
  }

  // ── what the folder watch says ─────────────────────────────────────────────

  watched(files: WatchedFile[], more: boolean) {
    this.n++;
    if (more) this.onDirs?.(null);
    else this.onDirs?.([...new Set(files.map((f) => (f.p.includes('/') ? f.p.slice(0, f.p.lastIndexOf('/')) : '')))]);
    this.host.reread(more ? null : files.map((f) => f.p));
    const now = Date.now();
    const list = files.filter((f) => !f.dir && !this.host.savedHere(f.p));
    const follow = this.host.running() && list.length <= MANY && !more;
    for (const f of list) {
      if (f.k === 'gone') {
        this.seen(f.p);
        if (f.p in this.disk) { const { [f.p]: _, ...rest } = this.disk; this.disk = rest; }
        if (f.p in this.order && !this.changes.has(f.p)) { const { [f.p]: _, ...rest } = this.order; this.order = rest; }
        continue;
      }
      this.#mark(f.p, f.k);
      // pi's own write: it is told of by pi, with its diff
      const own = now - (this.#toolAt.get(f.p) ?? 0) < 3000 || this.drafts.some((d) => d.key === f.p && !d.draft.shell);
      if (own) continue;
      this.disk = { ...this.disk, [f.p]: { k: this.marks[f.p] ?? f.k, n: (this.disk[f.p]?.n ?? 0) + 1, at: now } };
      this.order = { ...this.order, [f.p]: ++this.#count };
      if (follow) {
        this.aim = f.p;
        this.failed = null;
      }
    }
  }

  /** Connected again: what changed meanwhile was not told. */
  resync() {
    this.onDirs?.(null);
    this.host.reread(null);
  }

  dispose() {
    clearTimeout(this.#failTimer);
  }
}
