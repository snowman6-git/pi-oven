import { api, auth, AuthError, type Branched } from './api';
import { Chat } from './chat.svelte';
import { copyText } from './clipboard';
import { alertPerson, notifyEnabled, setNotifyEnabled } from './notify';
import type { RuntimeInfo, SessionSummary } from './protocol';

type Theme = 'auto' | 'light' | 'dark';
export type InspectorTab = 'info' | 'files' | 'diff' | 'git' | 'point';

function load<T>(key: string, fallback: T): T {
  try { const v = localStorage.getItem(key); return v == null ? fallback : (JSON.parse(v) as T); } catch { return fallback; }
}
function save(key: string, v: unknown) {
  try { localStorage.setItem(key, JSON.stringify(v)); } catch { /* private mode */ }
}

class App {
  status = $state<'booting' | 'auth' | 'down' | 'ready'>('booting');
  home = $state('');
  sessions = $state<SessionSummary[]>([]);
  chats = $state<Chat[]>([]);
  currentId = $state<string | null>(null);
  error = $state<string | null>(null);
  folderPicker = $state(false);
  flashText = $state<string | null>(null);
  /** A question that needs a yes/no (replaces the browser's confirm()). */
  dialog = $state<{ title: string; body?: string; items?: string[]; confirm: string; danger?: boolean; resolve: (ok: boolean) => void } | null>(null);
  /** Multi-select mode in the session list. */
  selecting = $state(false);
  picked = $state<string[]>([]);
  #flashTimer: ReturnType<typeof setTimeout> | undefined;
  settingsOpen = $state(false);
  /** The dialog for adding models and their endpoints. */
  modelDialog = $state(false);
  /** The session whose branches are being looked at; works for sessions that are not open. */
  treeTarget = $state<{ path: string; cwd: string; title: string } | null>(null);
  /** Phones and small tablets: panels are drawers instead of columns. */
  narrow = $state(false);
  inspectorDrawer = $state(false);
  inspectorTab = $state<InspectorTab>(load('pi-oven.inspector-tab', 'info'));
  inspectorW = $state(Math.min(760, Math.max(280, load('pi-oven.inspector-w', 340))));
  /** The session list folded to a strip of icons. */
  sidebarFolded = $state(load('pi-oven.sidebar-folded', false));
  /** Wider than the width where the list becomes a drawer. */
  wide = $state(true);
  foldAnim = $state(false);

  sidebarOpen = $state(false);
  inspectorOpen = $state(load('pi-oven.inspector', true));
  notifyOn = $state(notifyEnabled());
  theme = $state<Theme>(load('pi-oven.theme', 'dark'));

  get current(): Chat | undefined {
    return this.chats.find((c) => c.runtime.id === this.currentId);
  }

  /** Chat that has this session file open, if any. */
  chatFor(sessionPath: string): Chat | undefined {
    return this.chats.find((c) => c.sessionFile === sessionPath);
  }

  async boot() {
    this.watchViewport();
    auth.captureFromHash();
    this.applyTheme();
    try {
      const info = await api.info();
      this.home = info.home;
      this.status = 'ready';
    } catch (e) {
      this.status = e instanceof AuthError ? 'auth' : 'down';
      if (e instanceof AuthError) this.#lastAuth = e.why;
      return;
    }
    const [runtimes] = await Promise.all([api.runtimes(), this.refreshSessions()]);
    for (const r of runtimes) this.#attach(r);
    const wanted = new URLSearchParams(location.search).get('r');
    this.select(this.chats.find((c) => c.runtime.id === wanted)?.runtime.id ?? this.chats[0]?.runtime.id ?? null);
  }

  async submitToken(token: string) {
    auth.token = token.trim();
    this.status = 'booting';
    await this.boot();
    if ((this.status as string) === 'auth') this.error = this.#lastAuth === 'locked' ? 'locked' : 'wrong';
  }

  async refreshSessions() {
    try { this.sessions = await api.sessions(); } catch (e) { this.#fail(e); }
  }

  #attach(r: RuntimeInfo): Chat {
    const existing = this.chats.find((c) => c.runtime.id === r.id);
    if (existing) return existing;
    const chat = new Chat(r, this.#host());
    this.chats.push(chat);
    return chat;
  }

  /** What a session needs from the app around it. */
  #host() {
    return {
      changed: () => void this.refreshSessionsSoon(),
      notify: (c: Chat, kind: 'done' | 'ask') => this.#attention(c, kind),
      flash: (t: string) => this.flash(t),
      ask: (o: Parameters<App['ask']>[0]) => this.ask(o),
    };
  }

  ask(opts: { title: string; body?: string; items?: string[]; confirm: string; danger?: boolean }): Promise<boolean> {
    this.dialog?.resolve(false);
    return new Promise((resolve) => {
      this.dialog = { ...opts, resolve: (ok) => { this.dialog = null; resolve(ok); } };
    });
  }

  /** Sessions that are open in a tab belong to a running pi; close those first. */
  canDelete(s: SessionSummary) {
    return !this.chatFor(s.path);
  }

  startSelect(first?: SessionSummary) {
    this.selecting = true;
    this.picked = first && this.canDelete(first) ? [first.path] : [];
  }

  stopSelect() {
    this.selecting = false;
    this.picked = [];
  }

  togglePick(s: SessionSummary) {
    if (!this.canDelete(s)) return;
    this.picked = this.picked.includes(s.path) ? this.picked.filter((p) => p !== s.path) : [...this.picked, s.path];
  }

  pickMany(list: SessionSummary[], on: boolean) {
    const paths = list.filter((s) => this.canDelete(s)).map((s) => s.path);
    this.picked = on ? [...new Set([...this.picked, ...paths])] : this.picked.filter((p) => !paths.includes(p));
  }

  /** Ask once, then delete. Used for one session and for a checked selection alike. */
  async deleteSessions(list: SessionSummary[]) {
    const doomed = list.filter((s) => this.canDelete(s));
    if (!doomed.length) return;
    const titles = doomed.map((s) => s.name || s.preview || '제목 없는 세션');
    const ok = await this.ask({
      title: doomed.length === 1 ? '이 세션을 삭제할까요?' : `세션 ${doomed.length}개를 삭제할까요?`,
      body: '세션 기록 파일이 지워지며 되돌릴 수 없습니다.',
      items: titles,
      confirm: doomed.length === 1 ? '삭제' : `${doomed.length}개 삭제`,
      danger: true,
    });
    if (!ok) return;

    const results = await Promise.allSettled(doomed.map((s) => api.deleteSession(s.path)));
    const gone = new Set(doomed.filter((_, i) => results[i].status === 'fulfilled').map((s) => s.path));
    this.sessions = this.sessions.filter((s) => !gone.has(s.path));
    this.picked = this.picked.filter((p) => !gone.has(p));
    const failed = doomed.length - gone.size;
    if (failed) this.error = `${failed}개를 삭제하지 못했습니다.`;
    this.flash(failed ? `${gone.size}개 삭제, ${failed}개 실패` : `${gone.size}개 세션을 삭제했습니다`);
    if (this.selecting && (failed === 0 || this.picked.length === 0)) this.stopSelect();
  }

  flash(text: string) {
    this.flashText = text;
    clearTimeout(this.#flashTimer);
    this.#flashTimer = setTimeout(() => (this.flashText = null), 1800);
  }

  async copy(text: string, what = '복사했습니다') {
    this.flash((await copyText(text)) ? what : '복사하지 못했습니다');
  }

  /** Looking right at it? Then nothing to announce. */
  #attention(chat: Chat, kind: 'done' | 'ask') {
    const watching = !document.hidden && document.hasFocus() && this.currentId === chat.runtime.id;
    if (watching) return;
    chat.unseen = true;
    alertPerson(kind, chat.title || chat.runtime.cwd, () => this.select(chat.runtime.id));
  }

  get unseenCount() {
    return this.chats.filter((c) => c.unseen).length;
  }

  toggleNotify() {
    this.notifyOn = !this.notifyOn;
    setNotifyEnabled(this.notifyOn);
  }

  #sessionsTimer: ReturnType<typeof setTimeout> | undefined;
  refreshSessionsSoon() {
    clearTimeout(this.#sessionsTimer);
    this.#sessionsTimer = setTimeout(() => void this.refreshSessions(), 800);
  }

  select(id: string | null) {
    this.currentId = id;
    this.inspectorDrawer = false;
    const c = this.chats.find((x) => x.runtime.id === id);
    if (c) c.unseen = false;
    this.sidebarOpen = false;
    const url = new URL(location.href);
    id ? url.searchParams.set('r', id) : url.searchParams.delete('r');
    history.replaceState(null, '', url);
  }

  /** `/reload`: pi has no in-process reload over RPC, so restart it on the same session. */
  async restartChat(chat: Chat) {
    try {
      const rt = await api.restartRuntime(chat.runtime.id, chat.sessionFile);
      const at = this.chats.indexOf(chat);
      chat.dispose();
      const fresh = new Chat(rt, this.#host());
      at >= 0 ? this.chats.splice(at, 1, fresh) : this.chats.push(fresh);
      this.select(rt.id);
    } catch (e) { this.#fail(e); }
  }

  async openFolder(cwd: string) {
    try {
      const rt = await api.openRuntime({ cwd });
      this.select(this.#attach(rt).runtime.id);
    } catch (e) { this.#fail(e); }
  }

  async openSession(s: SessionSummary) {
    const open = this.chatFor(s.path);
    if (open) return this.select(open.runtime.id);
    try {
      const rt = await api.openRuntime({ cwd: s.cwd, sessionPath: s.path });
      this.select(this.#attach(rt).runtime.id);
    } catch (e) { this.#fail(e); }
  }

  /** Open the session file a branch just created, and put the message to rewrite in its composer. */
  async openBranch(b: Branched) {
    try {
      const rt = await api.openRuntime({ cwd: b.cwd, sessionPath: b.path });
      const chat = this.#attach(rt);
      this.select(chat.runtime.id);
      if (b.text != null) chat.prefill = { text: b.text, n: Date.now() };
      void this.refreshSessions();
    } catch (e) { this.#fail(e); }
  }

  async closeChat(chat: Chat) {
    if (chat.editors.dirtyCount > 0) {
      const ok = await this.ask({ title: '저장하지 않은 파일이 있습니다', body: '세션을 닫으면 편집 중이던 변경 내용이 사라집니다.', items: chat.editors.docs.filter((d) => d.dirty).map((d) => d.path), confirm: '버리고 닫기', danger: true });
      if (!ok) return;
    }
    chat.dispose();
    this.chats = this.chats.filter((c) => c !== chat);
    await api.closeRuntime(chat.runtime.id).catch(() => {});
    if (this.currentId === chat.runtime.id) this.select(this.chats[0]?.runtime.id ?? null);
    void this.refreshSessions();
  }

  /** The list as an icon strip: only where it is a column, never as the phone drawer. */
  get rail() {
    return this.wide && this.sidebarFolded;
  }

  toggleSidebarFold() {
    this.sidebarFolded = !this.sidebarFolded;
    save('pi-oven.sidebar-folded', this.sidebarFolded);
    if (this.sidebarFolded) this.stopSelect();
    this.foldAnim = true;
    setTimeout(() => (this.foldAnim = false), 260);
  }

  setInspectorTab(t: InspectorTab) {
    this.inspectorTab = t;
    save('pi-oven.inspector-tab', t);
  }

  /** Show the right panel on a given tab (from a menu item, for instance). */
  openInspector(t: InspectorTab) {
    this.setInspectorTab(t);
    if (this.narrow) this.inspectorDrawer = true;
    else if (!this.inspectorOpen) {
      this.inspectorOpen = true;
      save('pi-oven.inspector', true);
    }
  }

  setInspectorWidth(w: number, persist = false) {
    this.inspectorW = Math.min(760, Math.max(280, Math.round(w)));
    if (persist) save('pi-oven.inspector-w', this.inspectorW);
  }

  get showInspector() {
    return this.narrow ? this.inspectorDrawer : this.inspectorOpen;
  }

  toggleInspector() {
    if (this.narrow) {
      this.inspectorDrawer = !this.inspectorDrawer;
      return;
    }
    this.inspectorOpen = !this.inspectorOpen;
    save('pi-oven.inspector', this.inspectorOpen);
  }

  watchViewport() {
    const mq = window.matchMedia('(max-width: 1100px)');
    const sync = () => {
      this.narrow = mq.matches;
      if (!mq.matches) this.inspectorDrawer = false;
    };
    sync();
    mq.addEventListener('change', sync);
    const wq = window.matchMedia('(min-width: 901px)');
    const syncWide = () => (this.wide = wq.matches);
    syncWide();
    wq.addEventListener('change', syncWide);
  }

  setTheme(t: Theme) {
    this.theme = t;
    save('pi-oven.theme', t);
    this.applyTheme();
  }

  cycleTheme() {
    this.theme = this.theme === 'dark' ? 'light' : this.theme === 'light' ? 'auto' : 'dark';
    save('pi-oven.theme', this.theme);
    this.applyTheme();
  }

  applyTheme() {
    const el = document.documentElement;
    el.setAttribute('data-theme', this.theme);
  }

  #lastAuth: 'wrong' | 'locked' = 'wrong';

  #fail(e: unknown) {
    if (e instanceof AuthError) this.status = 'auth';
    else this.error = (e as Error).message === 'network' ? '서버에 연결하지 못했습니다.' : (e as Error).message;
  }
}

export const app = new App();

// Coming back to the tab counts as having seen the current session.
window.addEventListener('focus', () => {
  const c = app.current;
  if (c) c.unseen = false;
});

// Back from the background (a phone puts a page to sleep and its connections with it), or the network is back:
// check every connection now instead of waiting for the next retry to come round.
const wake = () => {
  if (document.hidden) return;
  for (const c of app.chats) c.conn.nudge();
};
document.addEventListener('visibilitychange', wake);
window.addEventListener('pageshow', wake);
window.addEventListener('online', wake);
