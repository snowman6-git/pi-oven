// The files open for editing in one session: which are open, whether each has unsaved changes, and
// keeping them honest against the disk. The text being typed lives in the editor (CodeMirror) itself;
// this holds what the disk had, the revision it was at, and what to do when the two disagree.

import { api, HttpError, type FileText } from './api';

export interface Doc {
  path: string;
  name: string;
  loading: boolean;
  error: string | null;
  /** What the disk held when last read or saved; the editor starts from this. */
  base: string;
  rev: string | null;
  eol: 'lf' | 'crlf';
  bom: boolean;
  dirty: boolean;
  /** The disk no longer matches what this was based on. */
  conflict: null | 'changed' | 'deleted';
  saving: boolean;
  /** Bumped whenever `base` is replaced from disk, so the editor knows to start over from it. */
  version: number;
}

/** What is selected in the file in view, for sending to pi along with a message. */
export interface Selection {
  path: string;
  /** 1-based lines; `text` is empty when nothing is selected (just the cursor). */
  from: number;
  to: number;
  text: string;
}

export interface EditorHost {
  flash(text: string): void;
  ask(o: { title: string; body?: string; items?: string[]; confirm: string; danger?: boolean }): Promise<boolean>;
}

/** Dragging a file from the explorer carries this (the path, and which session it belongs to). */
export const FILE_MIME = 'application/x-pi-oven-file';

function read<T>(key: string, fallback: T): T {
  try { const v = localStorage.getItem(key); return v == null ? fallback : (JSON.parse(v) as T); } catch { return fallback; }
}

const nameOf = (p: string) => p.slice(p.lastIndexOf('/') + 1);

const POLL_MS = 2500;

export class Editors {
  docs = $state<Doc[]>([]);
  /** The path of the file in view; null means the conversation. */
  active = $state<string | null>(null);
  /** Called when the conversation is brought back into view. */
  onChatShown: (() => void) | null = null;
  /** Chat and editor side by side (when there is room) instead of one at a time in tabs. */
  split = $state(read('pi-oven.split', true));
  /** How much of the width the conversation gets in a split. */
  splitFrac = $state(Math.min(0.7, Math.max(0.3, read('pi-oven.split-frac', 0.5))));
  /** The file that was last in view: what a split keeps showing when no tab is picked. */
  last = $state<string | null>(null);
  /** The file the editor is showing right now (set by the editor), and what is selected in it. */
  inView = $state<string | null>(null);
  selection = $state<Selection | null>(null);
  /** Send the open file along with the next message even with nothing selected. */
  attachFile = $state(false);
  /** A selection the person took off the next message (so it does not come back until the selection changes). */
  dismissed = $state<string | null>(null);
  /** The editor registers how to get the text typed in a file (it may not be the one in view). */
  textOf: ((path: string) => string | null) | null = null;
  /** …and is told when a save went through, so it can mark that text as the saved one. */
  onSaved: ((path: string, text: string) => void) | null = null;
  /** Brings a change made on disk into the editor's text. `applied`: done (n lines touched); `conflict`: it overlaps what was typed;
      `later`: the person is mid-composition; `none`: no editor holds this file yet. */
  applyRemote: ((path: string, f: FileText) => { result: 'applied'; lines: number } | { result: 'conflict' | 'later' | 'none' }) | null = null;

  #timer: ReturnType<typeof setInterval> | undefined;
  #soon: ReturnType<typeof setTimeout> | undefined;
  #polling = false;
  /** A revision seen on disk once: it is only taken in when it is seen again, so a file caught half-written is not. */
  #seen = new Map<string, string>();
  readonly #key: string;

  constructor(private runtimeId: string, cwd: string, private host: EditorHost) {
    this.#key = `pi-oven.editor:${cwd}`;
    this.#restore();
    this.#timer = setInterval(() => void this.poll(), POLL_MS);
  }

  dispose() {
    clearInterval(this.#timer);
    clearTimeout(this.#soon);
  }

  get dirtyCount() {
    return this.docs.filter((d) => d.dirty).length;
  }

  get(path: string) {
    return this.docs.find((d) => d.path === path);
  }

  // ── tabs ────────────────────────────────────────────────────────────────
  async open(path: string) {
    const have = this.get(path);
    if (have) {
      this.activate(path);
      return;
    }
    const doc: Doc = { path, name: nameOf(path), loading: true, error: null, base: '', rev: null, eol: 'lf', bom: false, dirty: false, conflict: null, saving: false, version: 0 };
    this.docs.push(doc);
    this.activate(path);
    await this.#load(path);
  }

  activate(path: string | null) {
    this.active = path;
    if (path) this.last = path;
    else this.onChatShown?.();
    this.#persist();
  }

  setSplit(on: boolean) {
    this.split = on;
    try { localStorage.setItem('pi-oven.split', JSON.stringify(on)); } catch { /* private mode */ }
  }

  setSplitFrac(f: number, persist = false) {
    this.splitFrac = Math.min(0.7, Math.max(0.3, f));
    if (persist) try { localStorage.setItem('pi-oven.split-frac', JSON.stringify(this.splitFrac)); } catch { /* private mode */ }
  }

  // ── sending what is in view along with a message ───────────────────────
  get selectionKey() {
    const s = this.selection;
    return s && s.path === this.inView && s.text ? `${s.path}:${s.from}-${s.to}` : null;
  }

  /** What would go with the next message: a selected range (with its text), or just the file when asked for. */
  get context(): { label: string; auto: boolean } | null {
    const p = this.inView;
    if (!p) return null;
    const s = this.selection;
    if (s && s.path === p && s.text && this.dismissed !== this.selectionKey) {
      return { label: `${p}:${s.from}${s.to > s.from ? '-' + s.to : ''}`, auto: true };
    }
    // nothing (more) selected to send: the file itself can still be offered
    return { label: p, auto: this.attachFile };
  }

  /** The text to add to the message, and forget it (a selection is sent once). */
  takeContext(): string {
    const c = this.context;
    const s = this.selection;
    this.attachFile = false;
    if (!c?.auto || !this.inView) return '';
    // a selection that was taken off the message does not come back just because the file was asked for
    if (!s || s.path !== this.inView || !s.text || this.dismissed === this.selectionKey) return `\n\n[편집기에서 열어 둔 파일: ${this.inView}]`;
    this.dismissed = this.selectionKey;
    const body = s.text.replace(/\n$/, ''); // a selection that runs to the start of the next line carries its last line break
    const cut = body.length > 30_000 ? body.slice(0, 30_000) + '\n…(이하 생략)' : body;
    const lang = s.path.includes('.') ? s.path.slice(s.path.lastIndexOf('.') + 1) : '';
    const fence = '`'.repeat(Math.max(3, ...[...cut.matchAll(/`+/g)].map((m) => m[0].length + 1)));
    return `\n\n[편집기에서 선택: ${c.label}]\n${fence}${lang}\n${cut}\n${fence}`;
  }

  async close(path: string, ask = true) {
    const doc = this.get(path);
    if (!doc) return true;
    if (ask && doc.dirty) {
      const ok = await this.host.ask({ title: '저장하지 않은 변경이 있습니다', body: `${doc.name}을(를) 닫으면 변경한 내용이 사라집니다.`, confirm: '버리고 닫기', danger: true });
      if (!ok) return false;
    }
    const i = this.docs.findIndex((d) => d.path === path);
    this.docs.splice(i, 1);
    if (this.active === path) this.active = this.docs[Math.min(i, this.docs.length - 1)]?.path ?? null;
    this.#persist();
    return true;
  }

  async closeOthers(path: string) {
    for (const d of [...this.docs]) if (d.path !== path) await this.close(d.path);
  }

  /** A file or folder was renamed: tabs inside it follow. */
  renamed(from: string, to: string) {
    for (const d of this.docs) {
      if (d.path === from || d.path.startsWith(from + '/')) {
        const was = d.path;
        d.path = to + d.path.slice(from.length);
        d.name = nameOf(d.path);
        if (this.active === was) this.active = d.path;
      }
    }
    this.#persist();
  }

  /** A file or folder was deleted from here: its tabs go with it. */
  gone(path: string) {
    for (const d of [...this.docs]) if (d.path === path || d.path.startsWith(path + '/')) void this.close(d.path, false);
  }

  // ── disk ────────────────────────────────────────────────────────────────
  async #load(path: string) {
    const doc = this.get(path);
    if (!doc) return;
    try {
      const f = await api.readFile(this.runtimeId, path);
      doc.base = f.content;
      doc.rev = f.rev;
      doc.eol = f.eol;
      doc.bom = f.bom;
      doc.dirty = false;
      doc.conflict = null;
      doc.error = null;
      doc.version++;
    } catch (e) {
      const st = e instanceof HttpError ? e.status : 0;
      doc.error = st === 415 ? '텍스트(UTF-8) 파일이 아니라서 편집할 수 없습니다.'
        : st === 413 ? '파일이 너무 커서(2MB 넘음) 열 수 없습니다.'
        : st === 404 ? '파일이 없습니다.'
        : st === 403 ? '이 파일은 열 수 없습니다.'
        : '파일을 읽지 못했습니다.';
    } finally {
      doc.loading = false;
    }
  }

  /** Throw away what was typed and take what the disk has. */
  async reload(path: string) {
    const doc = this.get(path);
    if (!doc) return;
    doc.loading = true;
    await this.#load(path);
  }

  async save(path: string, overwrite = false) {
    const doc = this.get(path);
    const text = this.textOf?.(path);
    if (!doc || doc.saving || doc.error || text == null) return;
    doc.saving = true;
    try {
      const r = await api.writeFile(this.runtimeId, { path, content: text, rev: doc.rev, overwrite, eol: doc.eol, bom: doc.bom });
      doc.rev = r.rev;
      doc.base = text;
      doc.dirty = false;
      doc.conflict = null;
      this.onSaved?.(path, text);
    } catch (e) {
      if (e instanceof HttpError && e.status === 409) doc.conflict = e.body?.rev === null ? 'deleted' : 'changed';
      else this.host.flash(`저장하지 못했습니다${e instanceof HttpError ? ` (${e.status})` : ''}`);
    } finally {
      doc.saving = false;
    }
  }

  setDirty(path: string, dirty: boolean) {
    const d = this.get(path);
    if (d && d.dirty !== dirty) d.dirty = dirty;
  }

  /** Look at the disk for every open file: what pi (or anything else) changed is brought in, not overwritten. */
  async poll() {
    if (this.#polling || document.hidden) return;
    const open = this.docs.filter((d) => !d.loading && !d.error);
    if (!open.length) return;
    this.#polling = true;
    let again = false;
    try {
      const now = await api.statFiles(this.runtimeId, open.map((d) => d.path));
      for (const d of open) {
        if (d.saving) continue;
        const st = now[d.path];
        if (st === undefined) continue;
        if (st === null) {
          d.conflict = 'deleted';
        } else if (st.rev !== d.rev) {
          // a writer that truncates and then writes shows a half-empty file for a moment: wait until the same revision shows twice
          if (this.#seen.get(d.path) !== st.rev) {
            this.#seen.set(d.path, st.rev);
            again = true;
            continue;
          }
          if (await this.#bringIn(d, st.rev)) this.#seen.delete(d.path);
        } else {
          this.#seen.delete(d.path);
          if (d.conflict === 'deleted') d.conflict = null; // it is back
        }
      }
    } catch { /* the next round will retry */ }
    finally {
      this.#polling = false;
    }
    if (again) this.pollSoon(600);
  }

  /** The disk has a newer version: take in the lines that changed. Returns false if it should be tried again. */
  async #bringIn(d: Doc, rev: string): Promise<boolean> {
    let f: FileText;
    try { f = await api.readFile(this.runtimeId, d.path); } catch { return true; }
    if (f.rev !== rev) return false; // it moved on while being read
    const r = this.applyRemote?.(d.path, f) ?? { result: 'none' as const };
    if (r.result !== 'applied') {
      if (r.result === 'later') return false;
      if (r.result === 'conflict') {
        d.conflict = 'changed';
        return true;
      }
      if (d.dirty) d.conflict = 'changed';
      else { await this.reload(d.path); this.host.flash(`${d.name}: 디스크에서 바뀐 내용을 불러왔습니다`); }
      return true;
    }
    d.base = f.content;
    d.rev = f.rev;
    d.eol = f.eol;
    d.bom = f.bom;
    d.conflict = null;
    this.host.flash(`${d.name}: ${r.lines}줄 바뀐 내용을 ${d.dirty ? '합쳤습니다' : '반영했습니다'}`);
    return true;
  }

  /** pi just ran something that may have written files. */
  pollSoon(ms = 350) {
    clearTimeout(this.#soon);
    this.#soon = setTimeout(() => void this.poll(), ms);
  }

  // ── memory across reloads (which tabs, not what was typed) ──────────────
  #persist() {
    try { localStorage.setItem(this.#key, JSON.stringify({ paths: this.docs.map((d) => d.path), active: this.active })); } catch { /* private mode */ }
  }

  #restore() {
    let saved: { paths?: string[]; active?: string | null } = {};
    try { saved = JSON.parse(localStorage.getItem(this.#key) ?? '{}'); } catch { /* ignore */ }
    for (const p of saved.paths ?? []) {
      this.docs.push({ path: p, name: nameOf(p), loading: true, error: null, base: '', rev: null, eol: 'lf', bom: false, dirty: false, conflict: null, saving: false, version: 0 });
      void this.#load(p).then(() => {
        // a file that has gone since is not worth a tab
        const d = this.get(p);
        if (d?.error === '파일이 없습니다.') void this.close(p, false);
      });
    }
    this.active = saved.active && (saved.paths ?? []).includes(saved.active) ? saved.active : null;
  }
}
