import { api } from './api';
import { orderBatch } from './bgorder';
import { COMMAND as TODO_COMMAND, STATE_ENTRY as TODO_STATE, TOOL as TODO_TOOL, fromDetails as todoFromDetails, latestFromItems as todoFromItems, type Task as TodoTask } from '../addons/todo';
import { Connection } from './connection.svelte';
import { Editors, type EditorHost } from './editor.svelte';
import { pinsFor } from './pins.svelte';
import { beep } from './notify';
import { SPEED_STATUS_KEY, StreamTimer, parseSpeedLine, type Speed } from './speed';
import type {
  BgTask, ExtensionRequest, ImagePart, ModelInfo, RuntimeInfo, SessionState, SessionStats,
  SlashCommand, ThinkingLevel, Usage, WireMessage,
} from './protocol';
import {
  applyResult, blocksFromContent, customLabel, itemsFromMessages, nextKey, taskItem, taskItemFromXml, userItem,
  type Block, type Item, type ToolBlock,
} from './transcript';
import { stripAnsi, textOf } from './util';

export { itemsFromMessages };
export type { Block, Item, ToolBlock };

export interface Toast { id: number; text: string; level: 'info' | 'warning' | 'error' }

export interface ChatHost extends EditorHost {
  changed(): void;
  /** A run finished or a question is waiting; the host decides whether anyone needs to be told. */
  notify(chat: Chat, kind: 'done' | 'ask'): void;
}

export type Permission = 'auto' | 'manual';
const PERMISSION_KEY = 'pi-oven.permission';
export const storedPermission = (): Permission => {
  try { return localStorage.getItem(PERMISSION_KEY) === 'manual' ? 'manual' : 'auto'; } catch { return 'auto'; }
};

type Dialog = Extract<ExtensionRequest, { method: 'select' | 'confirm' | 'input' | 'editor' }>;

export class Chat {
  runtime = $state<RuntimeInfo>() as RuntimeInfo;
  conn: Connection;
  /** The files open for editing here. */
  editors: Editors;
  /** What is pinned in this folder. */
  pins;
  /** The model's task list (the todo add-on), when the package is installed and has been used. */
  todo = $state<TodoTask[] | null>(null);
  /** When the wait for the model's next answer began (a message sent, a tool finished), for the "waiting" bubble. */
  waitSince = $state(0);
  /** A message was sent and pi has not said it started yet. */
  #sending = $state(false);
  /** The llama-infra package has reported at all: its "before the first token" is the prompt prefill. */
  speedSeen = $state(false);
  /** The generation and prompt rates the llama-infra package last measured (it writes them to a status line). */
  pkgSpeed = $state<{ prefill?: number; gen?: number; at: number } | null>(null);
  /** pi answered while the conversation was not in view (another tab was). */
  unreadReply = $state(false);
  /** Bumped when files may have changed under the explorer (pi ran a tool that writes). */
  filesRev = $state(0);

  items = $state<Item[]>([]);
  state = $state<SessionState | null>(null);
  stats = $state<SessionStats | null>(null);
  running = $state(false);
  compacting = $state(false);
  retry = $state<string | null>(null);
  loaded = $state(false);
  queue = $state<{ steering: string[]; followUp: string[] }>({ steering: [], followUp: [] });
  commands = $state<SlashCommand[]>([]);
  models = $state<ModelInfo[]>([]);
  levels = $state<ThinkingLevel[]>([]);
  statuses = $state<Record<string, string>>({});
  widgets = $state<Record<string, string[]>>({});
  dialogs = $state<Dialog[]>([]);
  toasts = $state<Toast[]>([]);
  stderr = $state<string[]>([]);
  prefill = $state<{ text: string; n: number } | null>(null);
  extTitle = $state<string | null>(null);
  permission = $state<Permission>('auto');
  /** From pi-background-tasks' status line ("bg 2 running · 1 failed · 1 done"). */
  bg = $state({ running: 0, failed: 0, stopped: 0, done: 0 });
  bgOpen = $state(false);
  /** The task to show opened when the list is: set by clicking its chip. */
  bgFocus = $state<string | null>(null);
  /** Every task of this session as the server last reported it. Each gets a chip of its own. */
  bgTasks = $state<BgTask[]>([]);
  /** Finished while someone was watching: the chip stays a little, so the result is not missed. */
  bgFresh = $state<string[]>([]);
  /** Task ids in the order they first showed up here. The row follows this, not the start time: tasks that start together can report back in any order, and the row must fill from the left as each arrives. */
  bgOrder = $state<string[]>([]);
  /** A chip whose task has ended hands itself over to the conversation: the effect flies it to its notice bubble,
      which stays hidden until it lands. */
  bgHandoffs = $state<{ n: number; id: string }[]>([]);
  bgHandoffHold = $state<string[]>([]);
  /** Chips that are on their way out (the notice never came): fading. */
  bgFading = $state<string[]>([]);
  /** Whether the conversation is on screen (not behind an open file): the chip only flies to a bubble that can be seen. */
  chatVisible = $state(true);
  /** Just started: its chip stays hidden until the ember lands on it. */
  bgLanding = $state<string[]>([]);
  /** Launches for the effect to play, oldest first (it plays them one at a time, in this order). */
  bgLaunches = $state<{ n: number; toolId: string; taskId: string | null }[]>([]);
  /** Tasks that just ended, oldest first; the chip pops once for each. */
  bgEvents = $state<{ n: number; id: string; status: BgTask['status'] }[]>([]);
  /** Finished (or asking) while the person was looking elsewhere. */
  unseen = $state(false);
  /** Pending history requests for message-less sessions are cheap; flag lets the UI show a skeleton. */
  loadError = $state<string | null>(null);

  #statsTimer: ReturnType<typeof setTimeout> | undefined;
  #toastSeq = 0;
  #metaLoaded = false;
  #loading = false;
  #queued: any[] = [];
  #skipTs: number | null = null;
  #replaying = false;
  #launchSeq = 0;
  /** When the request that the next answer is waiting on was made (performance.now()), and the stopwatch of the answer in flight. */
  #ref = 0;
  #timer: StreamTimer | undefined;
  #speedAt = 0;
  #bgTimer: ReturnType<typeof setTimeout> | undefined;
  #bgBaseline = false;
  #bgFetching = false;
  #bgEventSeq = 0;
  #noticeTimer: ReturnType<typeof setTimeout> | undefined;
  #handoffSeq = 0;
  /** Tool calls that can start background tasks and have not returned yet (toolCallId -> tool name). `bg_run` itself reports its task when it ends; a script that calls it (`codemode`) reports nothing, so its tasks are found on the list. */
  #launching = new Map<string, string>();
  /** Tasks that have just turned up and are not placed in the row yet (id -> the tool row they leave from). */
  #pending = new Map<string, { source: string }>();
  #pendingSince = 0;
  #placeTimer: ReturnType<typeof setTimeout> | undefined;

  constructor(runtime: RuntimeInfo, private host: ChatHost) {
    this.runtime = runtime;
    this.pins = pinsFor(runtime.cwd);
    this.editors = new Editors(runtime.id, runtime.cwd, host);
    this.editors.onChatShown = () => (this.unreadReply = false);
    this.conn = new Connection(
      runtime.id,
      (rec) => this.#intake(rec),
      () => void this.#open(),
    );
  }

  get hasTodo() { return this.commands.some((c) => c.name === TODO_COMMAND); }

  /** The list as the package saved it (survives compaction, and covers calls made through another tool). */
  #todoTimer: ReturnType<typeof setTimeout> | undefined;
  refreshTodo(delay = 0) {
    clearTimeout(this.#todoTimer);
    this.#todoTimer = setTimeout(async () => {
      const file = this.sessionFile;
      if (!file || !this.hasTodo) return;
      try {
        const { data } = await api.sessionState(file, TODO_STATE);
        const tasks = todoFromDetails(data);
        if (tasks) this.todo = tasks;
      } catch { /* the conversation's own copy stays */ }
    }, delay);
  }

  get hasPermissionExt() { return this.commands.some((c) => c.name === 'oven-permission'); }

  /** Records that arrive while history is loading are held, then applied in order. */
  async #open() {
    this.#loading = true;
    this.#queued = [];
    await this.load();
    const queued = this.#queued;
    this.#loading = false;
    this.#queued = [];
    this.#replaying = true;
    for (const rec of queued) this.#onRecord(rec);
    this.#replaying = false;
  }

  /** Chips showing now: what is running, and what ended while someone was looking. In the order they started. */
  get bgChips(): BgTask[] {
    const at = (id: string) => { const i = this.bgOrder.indexOf(id); return i < 0 ? Infinity : i; };
    return this.bgTasks.filter((t) => (t.status === 'running' || this.bgFresh.includes(t.id))).sort((x, y) => at(x.id) - at(y.id));
  }

  /** The chip goes away without a trace: the person has looked at it. */
  dismissBg(id: string) {
    this.bgFresh = this.bgFresh.filter((x) => x !== id);
  }

  /** pi's notice for a task has reached the conversation. If the task still has its chip and the conversation can be seen,
      the chip becomes the bubble (it flies there; the bubble stays hidden until it lands); otherwise the chip just goes. */
  #taskArrived(id: string) {
    if (this.chatVisible) beep('bg-bubble'); // the notice bubble enters the conversation
    this.#awaitingNotice.delete(id);
    if (!this.bgChips.some((t) => t.id === id)) return;
    if (!this.chatVisible || document.hidden || this.bgLanding.includes(id)) {
      this.dismissBg(id);
      return;
    }
    this.bgHandoffHold = [...this.bgHandoffHold, id];
    this.bgHandoffs = [...this.bgHandoffs.slice(-30), { n: ++this.#handoffSeq, id }];
    // Never leave a bubble hidden if nothing plays the flight.
    setTimeout(() => this.landed(id), 8000);
  }

  /** The bubble has arrived (or never will be flown to). */
  landed(id: string) {
    this.bgHandoffHold = this.bgHandoffHold.filter((x) => x !== id);
    this.dismissBg(id);
  }

  /** Finished chips whose notice is held back because the agent is busy (pi delivers it as a follow-up, after the run). */
  #awaitingNotice = new Set<string>();

  /** pi sends the notice right away when it is idle, but while it is working the notice waits for the end of the run, which can be
      long after the task. So a busy agent keeps the chip (up to 15 minutes); once it settles the notice has 10 s to come. */
  #expireUnlessBusy(id: string) {
    if (!this.bgFresh.includes(id)) return;
    if (!this.running) return this.#expire(id);
    this.#awaitingNotice.add(id);
    setTimeout(() => { if (this.#awaitingNotice.delete(id)) this.#expire(id); }, 15 * 60_000);
    if (!this.#noticeTimer) this.#pollNotice();
  }

  /** The notice only ever lands in the session file (the RPC stream never emits it), so while a task awaits its
      notice the tail of the file is read for a `custom_message` record. */
  #pollNotice() {
    if (this.#awaitingNotice.size === 0) return;
    const file = this.sessionFile;
    if (file)
      api
        .tailSession(file)
        .then(({ lines }) => {
          for (const line of lines) {
            let rec: any;
            try {
              rec = JSON.parse(line);
            } catch {
              continue;
            }
            if (rec.type !== 'custom_message' || rec.customType !== 'background-task-notification') continue;
            const id = String(rec.details?.id ?? '');
            if (!id || !this.#awaitingNotice.has(id)) continue;
            if (this.items.some((i) => i.kind === 'task' && i.id === id)) continue;
            const t = taskItem(rec);
            if (t) {
              this.items.push(t);
              this.#taskArrived(id); // the pop, and the card flying into the bubble
            }
          }
        })
        .catch(() => {});
    this.#noticeTimer = setTimeout(() => this.#pollNotice(), 1500);
  }

  /** The notice did not come: a finished chip does not stay for ever. */
  #expire(id: string) {
    if (!this.bgFresh.includes(id) || this.bgHandoffHold.includes(id)) return;
    this.bgFading = [...this.bgFading, id];
    setTimeout(() => {
      this.dismissBg(id);
      this.bgFading = this.bgFading.filter((x) => x !== id);
    }, 340);
  }

  /** The task list, and what changed in it since last time. While anything runs it asks again every couple of seconds. */
  async refreshBgTasks() {
    if (this.#bgFetching) return;
    this.#bgFetching = true;
    try {
      const list = await api.tasks(this.runtime.id);
      const before = new Map(this.bgTasks.map((t) => [t.id, t.status]));
      const first = !this.#bgBaseline; // what already ran before this page looked is not news
      this.#bgBaseline = true;
      // what was already there when this page looked: oldest first; what comes later is added as it arrives
      if (first) this.#remember(orderBatch(list).map((t) => t.id));
      for (const t of list) {
        const was = before.get(t.id);
        if (t.status !== 'running' && (was === 'running' || (!was && !first))) this.#bgEnded(t);
        else if (!was && !first && t.status === 'running') this.#arrive(t.id);
      }
      // a task the extension just announced may not be in the list yet
      const young = this.bgTasks.filter((t) => t.status === 'running' && !list.some((x) => x.id === t.id) && Date.now() - t.startTime < 10_000);
      this.bgTasks = [...list, ...young];
    } catch { /* the next round will retry */ }
    finally {
      this.#bgFetching = false;
    }
    clearTimeout(this.#bgTimer);
    if (this.bgTasks.some((t) => t.status === 'running')) this.#bgTimer = setTimeout(() => void this.refreshBgTasks(), 2000);
  }

  #remember(ids: string[]) {
    const fresh = ids.filter((id) => !this.bgOrder.includes(id));
    if (fresh.length) this.bgOrder = [...this.bgOrder, ...fresh];
  }

  /** A task has turned up, found on the list or reported by its tool. It is hidden at once (a card made
      now would exist before the effect meant to deliver it). Its place is decided a moment later, once no
      more are arriving: a script starting ten at once reports them in any order, over a stretch of time,
      and placing each as it came would fill the row in that order. After the pause they are placed
      together, in a fixed order, and sent to their chips one after another from the left. */
  #arrive(id: string, source = '') {
    if (this.bgOrder.includes(id)) return;
    const known = this.#pending.get(id);
    if (known) {
      if (source) known.source = source;
      return;
    }
    this.#pending.set(id, { source: source || [...this.#launching.keys()].at(-1) || '' });
    this.#hold(id);
    if (!this.#pendingSince) this.#pendingSince = Date.now();
    clearTimeout(this.#placeTimer);
    // wait for a quiet moment, but never longer than a second and a half in all
    this.#placeTimer = setTimeout(() => this.#place(), Math.max(0, Math.min(350, 1500 - (Date.now() - this.#pendingSince))));
  }

  #place() {
    const tasks = [...this.#pending.keys()].map((id) => this.bgTasks.find((t) => t.id === id)).filter((t): t is BgTask => !!t);
    const ordered = orderBatch(tasks);
    this.#remember(ordered.map((t) => t.id));
    for (const t of ordered) this.#fly(this.#pending.get(t.id)!.source, t.id);
    // a task that left the list meanwhile has nothing to land
    for (const id of this.#pending.keys()) if (!tasks.some((t) => t.id === id)) this.bgLanding = this.bgLanding.filter((x) => x !== id);
    this.#pending.clear();
    this.#pendingSince = 0;
    if (tasks.length && this.chatVisible) beep('bg-card'); // the card(s) land in the tool row
  }

  #hold(id: string) {
    if (!this.bgLanding.includes(id)) this.bgLanding = [...this.bgLanding, id];
    // Never leave a card hidden if nothing is there to land it (another session, a background tab).
    setTimeout(() => (this.bgLanding = this.bgLanding.filter((x) => x !== id)), 20_000);
  }

  /** Queue a launch for the effect. `toolId` is the tool row it leaves from (empty: the bottom of the conversation). */
  #fly(toolId: string, taskId: string | null) {
    this.bgLaunches = [...this.bgLaunches.slice(-60), { n: ++this.#launchSeq, toolId, taskId }];
  }

  #bgEnded(t: BgTask) {
    if (this.chatVisible) beep(t.status === 'completed' ? 'bg-done' : 'bg-stop');
    this.bgEvents = [...this.bgEvents.slice(-19), { n: ++this.#bgEventSeq, id: t.id, status: t.status }];
    if (!this.bgFresh.includes(t.id)) this.bgFresh = [...this.bgFresh, t.id];
    // A clean finish has its moment (the chip pops), then waits for pi's notice to take it into the conversation;
    // if none comes it goes by itself. A failure stays until someone has seen it.
    if (t.status === 'completed') setTimeout(() => this.#expireUnlessBusy(t.id), 10_000);
  }

  #announceLaunch(toolId: string, task?: BgTask) {
    if (task?.id) {
      this.bgTasks = [...this.bgTasks.filter((t) => t.id !== task.id), { ...task, status: task.status ?? 'running', startTime: task.startTime ?? Date.now() }];
      this.#arrive(task.id, toolId);
      if (this.chatVisible) beep('bg-start');
    }
    setTimeout(() => void this.refreshBgTasks(), 600);
  }

  #intake(rec: any) {
    const expanded: any[] = rec.type === 'oven_replay' ? rec.events : [rec];
    for (const r of expanded) {
      if (this.#loading) this.#queued.push(r);
      else this.#onRecord(r);
    }
  }

  setPermission(mode: Permission) {
    this.permission = mode;
    try { localStorage.setItem(PERMISSION_KEY, mode); } catch { /* private mode */ }
    // Extension commands run immediately, even mid-run, and never reach the model.
    this.conn.request({ type: 'prompt', message: `/oven-permission ${mode}` }).catch(() => {});
  }

  get link() { return this.conn.link; }
  get sessionFile() { return this.state?.sessionFile ?? this.runtime.sessionPath ?? undefined; }
  get contextPercent(): number | null { return this.stats?.contextUsage?.percent ?? null; }

  get title(): string {
    if (this.state?.sessionName) return this.state.sessionName;
    const first = this.items.find((i) => i.kind === 'user');
    return first && first.kind === 'user' && first.text ? first.text.split('\n')[0].slice(0, 80) : '';
  }

  // ── loading ──────────────────────────────────────────────────────────────
  async load() {
    try {
      const [state, msgs] = await Promise.all([
        this.conn.request<SessionState>({ type: 'get_state' }),
        this.conn.request<{ messages: WireMessage[] }>({ type: 'get_messages' }),
      ]);
      this.state = state;
      this.running = state.isStreaming;
      this.compacting = state.isCompacting;
      this.items = itemsFromMessages(msgs.messages);
      this.todo = todoFromItems(this.items);
      this.loaded = true;
      this.loadError = null;
      this.host.changed();
      void this.refreshStats();
      void this.refreshBgTasks();
      if (!this.#metaLoaded) void this.#loadMeta();
    } catch (e) {
      this.loadError = (e as Error).message;
    }
  }

  async #loadMeta() {
    this.#metaLoaded = true;
    const [cmds, models, levels] = await Promise.allSettled([
      this.conn.request<{ commands: SlashCommand[] }>({ type: 'get_commands' }),
      this.conn.request<{ models: ModelInfo[] }>({ type: 'get_available_models' }),
      this.conn.request<{ levels: ThinkingLevel[] }>({ type: 'get_available_thinking_levels' }),
    ]);
    if (cmds.status === 'fulfilled') { this.commands = cmds.value.commands; this.refreshTodo(); }
    if (models.status === 'fulfilled') this.models = models.value.models;
    if (levels.status === 'fulfilled') this.levels = levels.value.levels;
    const want = storedPermission();
    if (this.hasPermissionExt && want !== this.permission) this.setPermission(want);
  }

  async refreshStats() {
    try { this.stats = await this.conn.request<SessionStats>({ type: 'get_session_stats' }); } catch { /* process may be gone */ }
  }

  #scheduleStats() {
    clearTimeout(this.#statsTimer);
    this.#statsTimer = setTimeout(() => void this.refreshStats(), 400);
  }

  // ── commands ─────────────────────────────────────────────────────────────
  async send(text: string, images: ImagePart[] = [], mode: 'steer' | 'followUp' = 'steer') {
    const cmd: Record<string, unknown> = { type: 'prompt', message: text };
    if (images.length) cmd.images = images;
    if (this.running) cmd.streamingBehavior = mode;
    // Show that the model is being waited for right away, not only once pi reports that it started (a /command starts nothing).
    if (!this.running && !text.trim().startsWith('/')) {
      this.waitSince = Date.now();
      this.#sending = true;
      setTimeout(() => (this.#sending = false), 15_000);
    }
    try {
      await this.conn.request(cmd);
    } catch (e) {
      this.#sending = false;
      this.notice('error', (e as Error).message);
    }
  }

  /** True while the model has been asked and nothing of its answer is on screen yet (and no tool is running meanwhile). */
  get waiting(): boolean {
    if (!this.running && !this.#sending) return false;
    const last = this.items[this.items.length - 1];
    if (!last) return this.#sending;
    if (last.kind === 'assistant') {
      if (last.live) return last.blocks.length === 0;
      return !last.blocks.some((b) => b.type === 'tool' && (b.status === 'running' || b.status === 'pending'));
    }
    return true;
  }

  abort() { this.conn.request({ type: 'abort' }).catch(() => {}); }

  async setModel(m: ModelInfo) {
    try {
      await this.conn.request({ type: 'set_model', provider: m.provider, modelId: m.id });
      this.state = await this.conn.request<SessionState>({ type: 'get_state' });
      void this.refreshStats();
    } catch (e) { this.notice('error', (e as Error).message); }
  }

  /** Ask pi for its models again (after one was added to models.json). */
  async reloadModels(): Promise<ModelInfo[]> {
    try {
      const r = await this.conn.request<{ models: ModelInfo[] }>({ type: 'get_available_models' });
      this.models = r.models;
    } catch { /* the list stays as it was */ }
    return this.models;
  }

  async setThinking(level: ThinkingLevel) {
    try {
      await this.conn.request({ type: 'set_thinking_level', level });
      if (this.state) this.state.thinkingLevel = level;
    } catch (e) { this.notice('error', (e as Error).message); }
  }

  async rename(name: string) {
    try {
      await this.conn.request({ type: 'set_session_name', name });
      if (this.state) this.state.sessionName = name;
      this.host.changed();
    } catch (e) { this.notice('error', (e as Error).message); }
  }

  async compact(instructions?: string) {
    try {
      await this.conn.request({ type: 'compact', ...(instructions ? { customInstructions: instructions } : {}) });
    } catch (e) { this.notice('error', (e as Error).message); }
  }

  async newSession() {
    try {
      await this.conn.request({ type: 'new_session' });
      await this.load();
    } catch (e) { this.notice('error', (e as Error).message); }
  }

  /** Branch into a new session from the n-th user message (0-based).
      `rewrite`: the message is put back in the message box; `regenerate`: it is sent again as it was;
      `rewind`: just go back to before it. The session it came from is left as it was in every case. */
  async fork(userIndex: number, mode: 'rewrite' | 'regenerate' | 'rewind' = 'rewrite') {
    try {
      const { messages } = await this.conn.request<{ messages: { entryId: string; text: string }[] }>({ type: 'get_fork_messages' });
      const target = messages[userIndex];
      if (!target) throw new Error('이 메시지에서는 분기할 수 없습니다.');
      const res = await this.conn.request<{ text: string; cancelled: boolean }>({ type: 'fork', entryId: target.entryId });
      if (res.cancelled) return;
      await this.load();
      if (mode === 'rewrite') this.prefill = { text: res.text, n: Date.now() };
      else if (mode === 'regenerate') await this.send(res.text);
    } catch (e) { this.notice('error', (e as Error).message); }
  }

  respond(id: string, answer: { value: string } | { confirmed: boolean } | { cancelled: true }) {
    this.conn.send({ type: 'extension_ui_response', id, ...answer });
    this.dialogs = this.dialogs.filter((d) => d.id !== id);
  }

  notice(level: 'info' | 'warn' | 'error', text: string) {
    this.items.push({ kind: 'notice', key: nextKey(), level, text, ts: Date.now() });
  }

  toast(text: string, level: Toast['level'] = 'info') {
    const id = ++this.#toastSeq;
    this.toasts.push({ id, text, level });
    setTimeout(() => (this.toasts = this.toasts.filter((t) => t.id !== id)), 5000);
  }

  dispose() {
    clearTimeout(this.#statsTimer);
    clearTimeout(this.#bgTimer);
    clearTimeout(this.#todoTimer);
    clearTimeout(this.#placeTimer);
    this.editors.dispose();
    this.conn.dispose();
  }

  // ── event stream ─────────────────────────────────────────────────────────
  #liveAssistant(create = true, ts?: number) {
    const last = this.items[this.items.length - 1];
    if (last?.kind === 'assistant' && last.live) return last;
    if (!create) return undefined;
    this.items.push({ kind: 'assistant', key: nextKey(), blocks: [], live: true, ts: ts ?? Date.now() });
    return this.items[this.items.length - 1] as Extract<Item, { kind: 'assistant' }>;
  }

  #findTool(id: string): ToolBlock | undefined {
    for (let i = this.items.length - 1, seen = 0; i >= 0 && seen < 40; i--, seen++) {
      const it = this.items[i];
      if (it.kind !== 'assistant') continue;
      const hit = it.blocks.find((b): b is ToolBlock => b.type === 'tool' && b.id === id);
      if (hit) return hit;
    }
  }

  #knownTools(): Map<string, ToolBlock> {
    const m = new Map<string, ToolBlock>();
    for (const it of this.items) if (it.kind === 'assistant') for (const b of it.blocks) if (b.type === 'tool') m.set(b.id, b);
    return m;
  }

  #onRecord(rec: any) {
    switch (rec.type) {
      case 'oven_attached':
        this.stderr = rec.stderrTail ?? [];
        break;
      case 'oven_stderr':
        this.stderr = [...this.stderr.slice(-39), rec.line];
        break;
      case 'oven_exit':
        this.running = false;
        this.notice('error', `pi 프로세스가 종료되었습니다${rec.code != null ? ` (코드 ${rec.code})` : ''}.`);
        this.host.changed();
        break;
      case 'oven_lagged':
        void this.load();
        break;

      case 'agent_start':
        this.#ref = performance.now();
        if (!this.#sending) this.waitSince = Date.now(); // a message just sent has been waited on since it was sent
        this.#sending = false;
        this.running = true;
        this.host.changed();
        break;
      case 'agent_settled':
        this.running = false;
        this.#sending = false;
        // the notices held back by the run come now; whatever has none in 10 s was not going to get one
        for (const id of this.#awaitingNotice) setTimeout(() => { if (this.#awaitingNotice.delete(id)) this.#expire(id); }, 10_000);
        this.retry = null;
        for (const it of this.items) if (it.kind === 'assistant') it.live = false;
        this.host.changed();
        void this.refreshStats();
        void this.conn.request<SessionState>({ type: 'get_state' }).then((s) => (this.state = s)).catch(() => {});
        this.host.notify(this, 'done');
        break;
      case 'turn_end':
        this.#scheduleStats();
        break;

      case 'message_start':
        if (rec.message?.role === 'assistant') {
          const ts = rec.message.timestamp as number | undefined;
          // Replayed events for a message history already holds: ignore them through its message_end.
          if (ts != null && this.items.some((i) => i.kind === 'assistant' && !i.live && i.ts === ts)) this.#skipTs = ts;
          else {
            this.#liveAssistant(true, ts);
            // Only what happens in front of us is timed: replayed history arrives all at once.
            this.#timer = this.#replaying || this.#loading ? undefined : new StreamTimer(this.#ref || performance.now());
          }
        }
        break;
      case 'message_update':
        if (this.#skipTs == null) this.#onAssistantEvent(rec.assistantMessageEvent);
        break;
      case 'message_end':
        this.#onMessageEnd(rec.message as WireMessage);
        break;

      case 'tool_execution_start': {
        const t = this.#findTool(rec.toolCallId);
        if (t) { t.status = 'running'; t.args = rec.args ?? t.args; if (!this.#replaying) t.startedAt = Date.now(); }
        if (/^(bg_(run|delegate)|codemode)/.test(rec.toolName) && !this.#replaying) {
          this.#launching.set(rec.toolCallId, rec.toolName);
          setTimeout(() => this.#launching.delete(rec.toolCallId), 60_000);
        }
        break;
      }
      case 'tool_execution_update': {
        const t = this.#findTool(rec.toolCallId);
        if (t) t.output = textOf(rec.partialResult?.content);
        break;
      }
      case 'tool_execution_end': {
        this.#ref = performance.now(); // the next answer waits on this
        this.waitSince = Date.now();
        if (/^(edit|write|bash|codemode)/.test(rec.toolName) && !this.#replaying) { this.filesRev++; this.editors.pollSoon(); }
        if (rec.toolName === TODO_TOOL && !rec.isError) {
          const tasks = todoFromDetails(rec.result?.details);
          if (tasks) this.todo = tasks;
          else if (!this.#replaying) this.refreshTodo(400); // delete results carry no details: pull the file's truth
        } else if (rec.toolName === 'codemode' && !this.#replaying && this.hasTodo) this.refreshTodo(400);
        const t = this.#findTool(rec.toolCallId);
        if (t) {
          applyResult(t, { content: rec.result?.content, details: rec.result?.details, isError: rec.isError });
          t.durationMs = rec.durationMs;
        }
        if (/^bg_(run|delegate)/.test(rec.toolName)) {
          this.#launching.delete(rec.toolCallId);
          if (!rec.isError && !this.#replaying) this.#announceLaunch(rec.toolCallId, rec.result?.details?.task);
        } else if (rec.toolName === 'codemode') {
          this.#launching.delete(rec.toolCallId);
          // Whatever the script started is on the list by now: look, and once more for stragglers.
          if (!this.#replaying) { void this.refreshBgTasks(); setTimeout(() => void this.refreshBgTasks(), 1200); }
        }
        break;
      }

      case 'bash_execution_update': {
        const last = this.items[this.items.length - 1];
        if (last?.kind === 'bash' && last.running) last.output += rec.delta;
        break;
      }
      case 'queue_update':
        this.queue = { steering: rec.steering ?? [], followUp: rec.followUp ?? [] };
        break;
      case 'session_info_changed':
        if (this.state) this.state.sessionName = rec.name;
        this.host.changed();
        break;
      case 'thinking_level_changed':
        if (this.state) this.state.thinkingLevel = rec.level;
        break;

      case 'compaction_start':
        this.compacting = true;
        break;
      case 'compaction_end':
        this.compacting = false;
        if (rec.errorMessage) this.notice('error', `압축 실패: ${rec.errorMessage}`);
        else if (!rec.aborted && rec.result) void this.load();
        break;
      case 'auto_retry_start':
        this.retry = `${rec.attempt}/${rec.maxAttempts}`;
        this.notice('warn', `요청이 실패해 다시 시도합니다 (${rec.attempt}/${rec.maxAttempts}): ${rec.errorMessage}`);
        break;
      case 'auto_retry_end':
        this.retry = null;
        if (!rec.success) this.notice('error', `재시도 실패: ${rec.finalError ?? ''}`);
        break;

      case 'extension_ui_request':
        this.#onExtensionUi(rec as ExtensionRequest);
        break;
    }
  }

  #onAssistantEvent(ev: any) {
    if (!ev) return;
    const live = this.#liveAssistant();
    if (!live) return;
    const i: number = ev.contentIndex;
    if (this.#timer && (ev.type === 'text_delta' || ev.type === 'thinking_delta' || ev.type === 'toolcall_delta')) {
      const now = performance.now();
      this.#timer.delta(now, typeof ev.delta === 'string' ? ev.delta : '');
      // the line under the answer follows it, a few times a second rather than at every token
      if (now - this.#speedAt > 200) {
        this.#speedAt = now;
        live.speed = this.#timer.snapshot(now, ev.partial?.usage?.output);
      }
    }
    switch (ev.type) {
      case 'text_start': live.blocks[i] = { type: 'text', text: '' }; break;
      case 'thinking_start': live.blocks[i] = { type: 'thinking', text: '' }; break;
      case 'text_delta': {
        const b = live.blocks[i] ?? (live.blocks[i] = { type: 'text', text: '' });
        if (b.type === 'text') b.text += ev.delta;
        break;
      }
      case 'thinking_delta': {
        const b = live.blocks[i] ?? (live.blocks[i] = { type: 'thinking', text: '' });
        if (b.type === 'thinking') b.text += ev.delta;
        break;
      }
      case 'text_end': { const b = live.blocks[i]; if (b?.type === 'text') b.text = ev.content; break; }
      case 'thinking_end': { const b = live.blocks[i]; if (b?.type === 'thinking') b.text = ev.content; break; }
      case 'toolcall_start':
        live.blocks[i] = { type: 'tool', id: ev.id, name: ev.toolName, args: {}, status: 'pending', output: '' };
        break;
      case 'toolcall_end': {
        const b = live.blocks[i];
        if (b?.type === 'tool') b.args = ev.toolCall?.arguments ?? b.args;
        break;
      }
    }
  }

  #onMessageEnd(m: WireMessage) {
    switch (m.role) {
      case 'assistant': {
        if (this.#skipTs != null && this.#skipTs === m.timestamp) { this.#skipTs = null; return; }
        const live = this.#liveAssistant();
        if (!live) return;
        live.blocks = blocksFromContent(m.content, this.#knownTools());
        live.live = false;
        live.model = m.model;
        live.usage = m.usage;
        live.stop = m.stopReason;
        live.error = m.errorMessage;
        live.ts = m.timestamp ?? live.ts;
        if (this.#timer) {
          live.speed = this.#timer.finish(performance.now(), m.usage?.output);
          this.#timer = undefined;
        }
        if (!this.#replaying && this.editors.active !== null) this.unreadReply = true;
        break;
      }
      case 'user': {
        this.#ref = performance.now();
        const x = taskItemFromXml(textOf(m.content));
        if (x) {
          this.items.push({ kind: 'task', key: nextKey(), id: x.id, name: x.name, command: '', status: x.status, exitCode: undefined, durationMs: undefined, text: x.name, ts: m.timestamp });
          if (!this.#replaying) this.#taskArrived(x.id);
          break;
        }
        if (!this.items.some((i) => i.kind === 'user' && i.ts === m.timestamp)) this.items.push(userItem(m));
        break;
      }
      case 'toolResult': {
        const t = m.toolCallId ? this.#findTool(m.toolCallId) : undefined;
        if (t) applyResult(t, m);
        break;
      }
      case 'bashExecution':
        if (this.items.some((i) => i.kind === 'bash' && i.ts === m.timestamp)) break;
        this.items.push({
          kind: 'bash', key: nextKey(), command: m.command ?? '', output: m.output ?? '', exitCode: m.exitCode,
          cancelled: !!m.cancelled, truncated: !!m.truncated, running: false, ts: m.timestamp,
        });
        break;
      case 'custom': {
        const t = taskItem(m);
        if (t) {
          this.items.push(t);
          if (!this.#replaying && t.kind === 'task') this.#taskArrived(t.id);
        } else if (m.display) this.items.push({ kind: 'summary', key: nextKey(), label: customLabel(m.customType), text: textOf(m.content), ts: m.timestamp });
        break;
      }
    }
  }

  #onExtensionUi(r: ExtensionRequest) {
    switch (r.method) {
      case 'select': case 'confirm': case 'input': case 'editor':
        if (this.dialogs.some((d) => d.id === r.id)) break;
        this.dialogs.push(r);
        this.host.notify(this, 'ask');
        if ('timeout' in r && r.timeout) setTimeout(() => (this.dialogs = this.dialogs.filter((d) => d.id !== r.id)), r.timeout + 500);
        break;
      case 'notify':
        this.toast(stripAnsi(r.message), r.notifyType ?? 'info');
        break;
      case 'setStatus': {
        if (r.statusKey === 'background-tasks') {
          const t = r.statusText ? stripAnsi(r.statusText) : '';
          const n = (word: string) => Number(t.match(new RegExp(`(\\d+)\\s+${word}`))?.[1] ?? 0);
          const next = { running: n('running'), failed: n('failed'), stopped: n('stopped'), done: n('done') };
          const wasRunning = this.bg.running > 0;
          this.bg = next;
          void this.refreshBgTasks();
          if (wasRunning && next.running === 0) this.host.notify(this, 'done');
          break;
        }
        if (r.statusKey === SPEED_STATUS_KEY) {
          if (r.statusText) this.speedSeen = true;
          const v = r.statusText ? parseSpeedLine(stripAnsi(r.statusText)) : null;
          this.pkgSpeed = v && (v.gen || v.prefill) ? { ...v, at: performance.now() } : null;
          break;
        }
        if (r.statusKey === 'oven-permission') {
          this.permission = r.statusText === 'manual' ? 'manual' : 'auto';
          break;
        }
        const text = r.statusText ? stripAnsi(r.statusText).trim() : '';
        if (text && text !== '−') this.statuses[r.statusKey] = text;
        else delete this.statuses[r.statusKey];
        break;
      }
      case 'setWidget': {
        const lines = (r.widgetLines ?? []).map(stripAnsi).filter((l) => l.trim());
        if (lines.length) this.widgets[r.widgetKey] = lines;
        else delete this.widgets[r.widgetKey];
        break;
      }
      case 'setTitle':
        this.extTitle = stripAnsi(r.title);
        break;
      case 'set_editor_text':
        this.prefill = { text: r.text, n: Date.now() };
        break;
    }
  }
}
