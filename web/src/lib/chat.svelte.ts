import { api } from './api';
import { limits } from './limits.svelte';
import { settings } from './settings.svelte';
import { providerName, resetDate, windowName } from './limits';
import { BATON_NOTE, type BatonNote } from './baton';
import { orderBatch } from './bgorder';
import { COMMAND as TODO_COMMAND, STATE_ENTRY as TODO_STATE, TOOL as TODO_TOOL, fromDetails as todoFromDetails, latestFromItems as todoFromItems, type Task as TodoTask } from '../addons/todo';
import { Connection } from './connection.svelte';
import { Subagents } from './subagents.svelte';
import { COMMAND as SA_COMMAND } from '../addons/subagents';
import { COMMAND as LENS_COMMAND } from '../addons/lens';
import { Editors, type EditorHost } from './editor.svelte';
import { pinsFor } from './pins.svelte';
import { StreamTimer, type Speed } from './speed';
import type {
  BgTask, ExtensionRequest, ImagePart, ModelInfo, RuntimeInfo, SessionState, SessionStats,
  SlashCommand, ThinkingLevel, Usage, WireMessage,
} from './protocol';
import {
  applyResult, blocksFromContent, customLabel, itemsFromMessages, nextKey, repeatsFailure, subagentItem, taskItem, taskItemFromXml, userItem,
  type Block, type Item, type ToolBlock,
} from './transcript';
import { stripAnsi, textOf } from './util';
import { t } from './i18n';

export { itemsFromMessages };
export type { Block, Item, ToolBlock };

export interface Toast { id: number; text: string; level: 'info' | 'warning' | 'error' }

export interface ChatHost extends EditorHost {
  changed(): void;
  /** The server no longer has this pi (it restarted): open the same session again in a new one. */
  gone(chat: Chat): void;
  /** This pi ended; another device may have restarted it on the same session (then follow it there). */
  exited(chat: Chat): void;
  /** pi offered updates (the server answered the prompt): the list shows them. */
  updates(): void;
  /** A run finished or a question is waiting; the host decides whether anyone needs to be told. */
  notify(chat: Chat, kind: 'done' | 'ask'): void;
}

export type Permission = 'auto' | 'manual';
const PERMISSION_KEY = 'pi-oven.permission';
export const storedPermission = (): Permission => {
  try { return localStorage.getItem(PERMISSION_KEY) === 'manual' ? 'manual' : 'auto'; } catch { return 'auto'; }
};

type Dialog = Extract<ExtensionRequest, { method: 'select' | 'confirm' | 'input' | 'editor' }>;

/** A reload gives every item a new key, which would rebuild the whole conversation on screen. Items that are the
    same message as before (same place, kind and time) keep their old key, so only what changed is drawn again. */
function keepKeys(old: Item[], next: Item[]): Item[] {
  for (let i = 0; i < next.length && i < old.length; i++) {
    const a = old[i], b = next[i];
    if (a.kind !== b.kind || !('ts' in a) || !('ts' in b) || a.ts == null || a.ts !== b.ts) break;
    (b as { key: number }).key = a.key;
  }
  return next;
}

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
  /** The 100h mode's place for this session: `at` is when the next try is due, empty while one is under way. */
  long = $state<{ attempt: number; max: number; at: number | null; error: string } | null>(null);
  /** Waiting for a usage limit to reset, to send `text` then (kept by the server). */
  resume = $state<{ at: number; text: string; tries: number; window: number | null; known: boolean } | null>(null);
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
  /** How far the effects have played each queue. Kept here, not in the effect: it is made again whenever this
      session comes back on screen, and would otherwise play every flight of the session again. */
  played = { launch: 0, event: 0, handoff: 0, saLaunch: 0, saEvent: 0, saHand: 0 };
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

  /** Subagent runs (pi-subagents): chips, tray and their notices. */
  sa: Subagents;

  constructor(runtime: RuntimeInfo, private host: ChatHost) {
    this.runtime = runtime;
    this.sa = new Subagents({
      enabled: () => this.hasSubagents,
      sessionFile: () => this.sessionFile,
      visible: () => this.chatVisible && !document.hidden,
      addNotice: (rec) => {
        const it = subagentItem({ ...rec, timestamp: typeof rec.timestamp === 'string' ? Date.parse(rec.timestamp) : rec.timestamp });
        if (!it || it.kind !== 'subagent') return null;
        const ids = it.notice.runIds;
        const dup = this.items.some((i) => i.kind === 'subagent' && (ids.length ? i.notice.runIds.some((r) => ids.includes(r)) : i.notice.text === it.notice.text));
        if (dup) return null;
        this.items.push(it);
        return it.key;
      },
    });
    this.pins = pinsFor(runtime.cwd);
    this.editors = new Editors(runtime.id, runtime.cwd, host);
    this.editors.onChatShown = () => (this.unreadReply = false);
    this.conn = new Connection(
      runtime.id,
      (rec) => this.#intake(rec),
      (again) => void this.#open(again),
    );
  }

  get hasTodo() { return this.commands.some((c) => c.name === TODO_COMMAND); }
  /** pi-subagents is installed (the add-on for it: chips, tray, notices). */
  get hasSubagents() { return this.commands.some((c) => c.name === SA_COMMAND); }
  get hasLens() { return this.commands.some((c) => c.name === LENS_COMMAND); }

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
  async #open(again = false) {
    this.#loading = true;
    this.#queued = [];
    // connected again (a phone that slept, a dropped link): only the history that came meanwhile
    await this.load(again);
    const queued = this.#queued;
    this.#loading = false;
    this.#queued = [];
    this.#replaying = true;
    for (const rec of queued) this.#onRecord(rec);
    this.#replaying = false;
    this.#settleNotices();
  }

  /** A notice that came with history (a reload, a reconnect, records replayed meanwhile) is not met live, so a chip
      still waiting for it would wait for ever: hand those over now. */
  #settleNotices() {
    for (const i of this.items) {
      if (i.kind !== 'task' || this.#noticed.has(i.id)) continue;
      if (this.bgFresh.includes(i.id) || this.#awaitingNotice.has(i.id)) this.#taskArrived(i.id, i.status);
    }
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
  #taskArrived(id: string, status?: string) {
    this.#awaitingNotice.delete(id);
    this.#keepUntilSeen.delete(id);
    // pi's notice is the word that the task is over: the list (read every 2 s) can lag behind it, and saying
    // "running" after it brought the chip back as a running one, which then waited for a notice that had come
    this.#noticed.add(id);
    const task = this.bgTasks.find((t) => t.id === id);
    const had = this.bgChips.some((t) => t.id === id);
    if (task?.status === 'running') {
      const ended = (['completed', 'failed', 'killed'].includes(status ?? '') ? status : 'completed') as BgTask['status'];
      this.bgTasks = this.bgTasks.map((t) => (t.id === id ? { ...t, status: ended } : t));
      if (had) {
        // its ending was not seen yet: the chip pops as for any ending, then flies to the notice
        this.bgEvents = [...this.bgEvents.slice(-19), { n: ++this.#bgEventSeq, id, status: ended }];
        if (!this.bgFresh.includes(id)) this.bgFresh = [...this.bgFresh, id];
      }
    }
    if (!had) return;
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

  /** Tasks whose notice has come: over, whatever the list still says. */
  #noticed = new Set<string>();
  /** Finished chips waiting for pi's notice (it may come at once, or after the run if the agent is busy). */
  #awaitingNotice = new Set<string>();
  /** Of those, the ones that stay until seen even if no notice comes (a failure). */
  #keepUntilSeen = new Set<string>();

  /** A task ended: look for its notice from now on (it can land within a second, while the agent is idle, and be
      over before any later check). A clean finish goes by itself if no notice comes: 10 s after the end when idle,
      else up to 15 minutes, and 10 s after the agent settles. A failure waits for its notice but never goes by itself. */
  #awaitNotice(id: string, keep: boolean) {
    this.#awaitingNotice.add(id);
    if (keep) this.#keepUntilSeen.add(id);
    if (!this.#noticeTimer) this.#pollNotice();
    if (keep) return;
    setTimeout(() => { if (!this.running && this.#awaitingNotice.delete(id)) this.#expire(id); }, 10_000);
    setTimeout(() => { if (this.#awaitingNotice.delete(id)) this.#expire(id); }, 15 * 60_000);
  }

  /** pi's notice comes on the RPC stream as the custom message's `message_end` (checked with pi 1.5.2, idle or busy),
      which ends the chip at once (`#taskArrived`). A page that was away for it reads the tail of the session file for
      the `custom_message` record instead, while a task awaits its notice. */
  #pollNotice() {
    // nothing left to wait for: clear the mark, or the next task would think a poll is still running and never start one
    if (this.#awaitingNotice.size === 0) {
      this.#noticeTimer = undefined;
      return;
    }
    const file = this.sessionFile;
    if (file)
      api
        .tailSession(file, 'background-task-notification')
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
            // already in the conversation (a history load brought it): only the chip is left to hand over
            const shown = this.items.find((i) => i.kind === 'task' && i.id === id);
            if (shown) {
              this.#taskArrived(id, shown.kind === 'task' ? shown.status : undefined);
              continue;
            }
            const t = taskItem(rec);
            if (t) {
              this.items.push(t);
              this.#taskArrived(id, t.kind === 'task' ? t.status : undefined); // the pop, and the card flying into the bubble
            }
          }
        })
        .catch(() => {});
    this.#noticeTimer = this.#awaitingNotice.size ? setTimeout(() => this.#pollNotice(), 1500) : undefined;
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
      // a task whose notice came is over even while the list still says it runs
      const list0 = list;
      const fixed = list0.map((t) => (this.#noticed.has(t.id) && t.status === 'running' ? { ...t, status: (before.get(t.id) ?? 'completed') as BgTask['status'] } : t));
      for (const t of fixed) {
        const was = before.get(t.id);
        if (this.#noticed.has(t.id)) continue; // its ending was handled when the notice came
        if (t.status !== 'running' && (was === 'running' || (!was && !first))) this.#bgEnded(t);
        else if (!was && !first && t.status === 'running') this.#arrive(t.id);
      }
      // a task the extension just announced may not be in the list yet
      const young = this.bgTasks.filter((t) => t.status === 'running' && !list.some((x) => x.id === t.id) && Date.now() - t.startTime < 10_000);
      this.bgTasks = [...fixed, ...young];
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
    this.bgEvents = [...this.bgEvents.slice(-19), { n: ++this.#bgEventSeq, id: t.id, status: t.status }];
    if (!this.bgFresh.includes(t.id)) this.bgFresh = [...this.bgFresh, t.id];
    // A clean finish has its moment (the chip pops), then waits for pi's notice to take it into the conversation;
    // if none comes it goes by itself. A failure stays until someone has seen it.
    this.#awaitNotice(t.id, t.status !== 'completed');
  }

  #announceLaunch(toolId: string, task?: BgTask) {
    if (task?.id) {
      this.bgTasks = [...this.bgTasks.filter((t) => t.id !== task.id), { ...task, status: task.status ?? 'running', startTime: task.startTime ?? Date.now() }];
      this.#arrive(task.id, toolId);
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
  #loadSeq = 0;
  /** pi's messages as this page has them (the last full answer, and every message that ended since), so a reconnect
      can ask for only what came after: the server answers with the rest (`from`) when its history still starts the
      same, and with all of it otherwise. */
  #raw: WireMessage[] = [];

  /** A message ended in front of us: it is in pi's history now. */
  #rawAdd(m: WireMessage | undefined) {
    if (!m) return;
    const seen = this.#raw.slice(-8).some((r) => r.timestamp === m.timestamp && r.role === m.role);
    if (!seen) this.#raw.push(m);
  }

  async load(since = false) {
    const seq = ++this.#loadSeq;
    try {
      const last = this.#raw[this.#raw.length - 1];
      const ask: Record<string, unknown> = { type: 'get_messages' };
      if (since && last?.timestamp != null) ask.oven_since = { n: this.#raw.length, ts: last.timestamp };
      const [state, msgs] = await Promise.all([
        this.conn.request<SessionState>({ type: 'get_state' }),
        this.conn.request<{ messages: WireMessage[]; from?: number }>(ask),
      ]);
      if (seq !== this.#loadSeq) return; // a later load is under way: an older answer must not overwrite it
      this.#takeState(state);
      this.running = state.isStreaming;
      this.compacting = state.isCompacting;
      this.#raw = msgs.from != null ? [...this.#raw.slice(0, msgs.from), ...msgs.messages] : msgs.messages;
      const next = itemsFromMessages(this.#raw);
      // a Baton Touch line whose note is not in the history yet (pi was busy: it comes with the next turn) stays
      const inHistory = new Set(next.flatMap((i) => (i.kind === 'baton' ? [this.#batonSig(i.note)] : [])));
      this.#batonLive = this.#batonLive.filter((b) => !inHistory.has(b.sig));
      next.push(...this.#batonLive.map((b): Item => ({ kind: 'baton', key: b.key, note: b.note, ts: b.ts })));
      this.items = keepKeys(this.items, next);
      this.todo = todoFromItems(this.items);
      this.sa.restore(this.items);
      this.#settleNotices();
      this.loaded = true;
      this.loadError = null;
      this.host.changed();
      void this.refreshStats();
      void this.refreshBgTasks();
      if (!this.#metaLoaded) void this.#loadMeta();
    } catch (e) {
      if (seq === this.#loadSeq) this.loadError = (e as Error).message;
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
  }

  #rereadTimer: ReturnType<typeof setTimeout> | undefined;
  #rereadSoon() {
    clearTimeout(this.#rereadTimer);
    this.#rereadTimer = setTimeout(() => {
      void this.refreshStats();
      void this.conn.request<SessionState>({ type: 'get_state' }).then((st) => this.#takeState(st)).catch(() => {});
    }, 400);
  }

  /** pi's state, and the queue checked against it: the queue on screen is built from `queue_update` events, and one
      missed (a dropped connection, a slow phone the server had to skip events for) would leave a message there that
      pi has long delivered. pi's own count is the truth: none pending, nothing queued; fewer, the oldest went. */
  #takeState(s: SessionState) {
    this.state = s;
    const n = s.pendingMessageCount;
    if (typeof n !== 'number') return;
    const { steering, followUp } = this.queue;
    const extra = steering.length + followUp.length - n;
    if (extra <= 0) return;
    const drop = Math.min(extra, steering.length);
    this.queue = { steering: steering.slice(drop), followUp: followUp.slice(extra - drop) };
  }

  /** `err` is already on screen: the newest reply's error block says it. */
  #shownError(err?: string): boolean {
    if (!err) return false;
    for (let i = this.items.length - 1; i >= Math.max(0, this.items.length - 6); i--) {
      const it = this.items[i];
      if (it.kind === 'assistant') return it.error === err;
    }
    return false;
  }

  /** Baton Touch lines shown as the server said so, before (or without) the session's own note of them: the note can
      wait for pi's next turn when pi was busy retrying. Keyed by what they say, so the note does not draw it twice,
      and kept across a reload of the history until the note is in it. */
  #batonLive: { sig: string; note: BatonNote; key: number; ts: number }[] = [];
  #batonSig(n: BatonNote) {
    return JSON.stringify([n.provider ?? null, n.email ?? null, n.model ?? null, n.at ?? null, !!n.back, n.was ?? null]);
  }
  #batonArrived(n: BatonNote, fromNote: boolean, ts?: number) {
    const sig = this.#batonSig(n);
    const live = this.#batonLive.find((b) => b.sig === sig);
    if (fromNote) {
      if (live) {
        this.#batonLive = this.#batonLive.filter((b) => b !== live);
        // already drawn when the server said so (by key: the list holds reactive copies, not the objects pushed)
        if (this.items.some((i) => i.key === live.key)) return;
      }
      this.items.push({ kind: 'baton', key: nextKey(), note: n, ts });
      return;
    }
    if (live) return;
    const key = nextKey(), now = Date.now();
    this.items.push({ kind: 'baton', key, note: n, ts: now });
    this.#batonLive = [...this.#batonLive.filter((b) => now - b.ts < 3_600_000), { sig, note: n, key, ts: now }];
  }

  /** A message of the person's arrived in the conversation: it is no longer waiting (also when pi's own match missed). */
  #delivered(m: WireMessage | undefined) {
    if (m?.role !== 'user') return;
    const text = textOf(m.content).trim();
    if (!text) return;
    const { steering, followUp } = this.queue;
    const i = steering.findIndex((q) => q.trim() === text);
    if (i >= 0) this.queue = { steering: steering.filter((_, k) => k !== i), followUp };
    else {
      const j = followUp.findIndex((q) => q.trim() === text);
      if (j >= 0) this.queue = { steering, followUp: followUp.filter((_, k) => k !== j) };
    }
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
      const r = await this.conn.request<{ held?: unknown } | undefined>(cmd);
      // the plan is used up: Zero-Waste keeps the message and sends it at the reset (the strip above shows it)
      if (r?.held) { this.#sending = false; this.waitSince = 0; }
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

  /** Send now: stop the run, and send what was waiting in the queue together with this, as one new message.
      pi's abort waits until the run has settled, so the prompt that follows starts a fresh run instead of queueing. */
  async sendNow(text: string, images: ImagePart[] = []) {
    let waiting: string[] = [];
    try {
      const q = await this.conn.request<{ steering?: string[]; followUp?: string[] }>({ type: 'clear_queue' });
      waiting = [...(q?.steering ?? []), ...(q?.followUp ?? [])];
    } catch { /* nothing was queued, or an older pi: send what was typed */ }
    const message = [...waiting, text].filter((t) => t.trim()).join('\n\n');
    if (!message && !images.length) return;
    try {
      await this.conn.request({ type: 'abort' });
    } catch (e) {
      this.notice('error', (e as Error).message);
      return;
    }
    await this.send(message, images);
  }

  async setModel(m: ModelInfo) {
    try {
      await this.conn.request({ type: 'set_model', provider: m.provider, modelId: m.id });
      this.#takeState(await this.conn.request<SessionState>({ type: 'get_state' }));
      void this.refreshStats();
      void this.#recommendedLevel(m);
    } catch (e) { this.notice('error', (e as Error).message); }
  }

  /** A model was picked: if its provider recommends a reasoning level for it (ChatGPT does), start from that one.
      Where the provider says nothing, the level stays as it was: no level is made up here. */
  async #recommendedLevel(m: ModelInfo) {
    try {
      const [rec, lv] = await Promise.all([
        api.recommendedLevels(),
        this.conn.request<{ levels: ThinkingLevel[] }>({ type: 'get_available_thinking_levels' }),
      ]);
      this.levels = lv.levels;
      const want = rec[m.provider]?.[m.id] as ThinkingLevel | undefined;
      if (want && lv.levels.includes(want) && this.state?.thinkingLevel !== want) await this.setThinking(want);
    } catch { /* no recommendation: keep the level */ }
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
      if (!target) throw new Error(t('chatmsg.noFork'));
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

  dismissToast(id: number) {
    this.toasts = this.toasts.filter((t) => t.id !== id);
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
    this.sa.dispose();
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

  #setLong(s: { attempt: number; max: number; inMs: number | null; error: string } | null | undefined) {
    this.long = s ? { attempt: s.attempt, max: s.max, at: s.inMs == null ? null : Date.now() + s.inMs, error: s.error } : null;
  }

  #setResume(s: { inMs: number; text: string; tries: number; window?: number | null; known?: boolean } | null | undefined) {
    this.resume = s ? { at: Date.now() + s.inMs, text: s.text, tries: s.tries, window: s.window ?? null, known: s.known ?? true } : null;
  }

  async resumeAct(action: 'now' | 'stop') {
    try { await api.resumeAct(this.runtime.id, action); } catch (e) { this.notice('error', (e as Error).message); }
  }

  async longAct(action: 'now' | 'stop') {
    try { await api.longAct(this.runtime.id, action); } catch (e) { this.notice('error', (e as Error).message); }
  }

  #onRecord(rec: any) {
    switch (rec.type) {
      case 'oven_attached':
        this.stderr = rec.stderrTail ?? [];
        this.#setLong(rec.long);
        this.#setResume(rec.resume);
        break;
      case 'oven_resume':
        this.#setResume(rec.state);
        void limits.refresh(); // a limit just hit marks its window as used up
        if (rec.gaveUp) this.notice('error', t('resume.gaveUp'));
        break;
      case 'oven_baton':
        // Baton Touch handed over: to another account (now, or `at` when every one is limited), to another provider's
        // model, or back to the model it came from. Drawn now; the session's own note of it (which may wait for pi's
        // next turn) then finds it drawn.
        this.#batonArrived(rec as BatonNote, false);
        if (rec.model) void this.conn.request<SessionState>({ type: 'get_state' }).then((st) => this.#takeState(st)).catch(() => {});
        if (!rec.at) limits.swapped(rec.provider, rec.email ?? rec.model);
        break;
      case 'oven_account':
        // another session's Baton Touch, or Settings, changed the account every session uses
        this.notice('info', t('accounts.elsewhere', { provider: providerName(rec.provider), who: rec.email || rec.provider }));
        limits.swapped(rec.provider, rec.email ?? rec.provider);
        break;
      case 'oven_long':
        this.#setLong(rec.state);
        if (rec.gaveUp) this.notice('error', t('long.gaveUp'));
        break;
      case 'oven_unheld':
        // the wait was stopped: the messages it held go back into the message box, not lost
        this.prefill = { text: rec.text, n: Date.now() };
        break;
      case 'oven_halt':
        // the server stopped a run whose requests kept failing back to back (a loop, not pi's spaced retries)
        this.notice('error', t('chatmsg.halted', { n: rec.fails }));
        break;
      case 'oven_stderr':
        this.stderr = [...this.stderr.slice(-39), rec.line];
        break;
      case 'oven_exit':
        this.running = false;
        this.long = null;
        this.resume = null;
        this.notice('error', t('chatmsg.exited', { code: rec.code != null ? t('chatmsg.code', { n: rec.code }) : '' }));
        this.host.changed();
        this.host.exited(this);
        break;
      case 'oven_update':
        this.host.updates();
        break;
      case 'oven_gone':
        this.running = false;
        this.long = null;
        this.resume = null;
        this.host.gone(this);
        break;
      case 'oven_lagged':
        // the server skipped events for this page and closes the socket: the reconnect loads and replays afresh
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
        // pi delivers or drops what was queued before a run settles
        this.queue = { steering: [], followUp: [] };
        this.#sending = false;
        // the notices held back by the run come now; whatever has none in 10 s was not going to get one
        for (const id of this.#awaitingNotice) if (!this.#keepUntilSeen.has(id)) setTimeout(() => { if (this.#awaitingNotice.delete(id)) this.#expire(id); }, 10_000);
        this.retry = null;
        for (const it of this.items) if (it.kind === 'assistant') it.live = false;
        this.host.changed();
        void this.refreshStats();
        void this.conn.request<SessionState>({ type: 'get_state' }).then((s) => this.#takeState(s)).catch(() => {});
        if (!this.long) this.host.notify(this, 'done'); // an hourly try that failed is not the end of anything
        break;
      case 'turn_end':
        this.#scheduleStats();
        break;

      case 'message_start':
        this.#delivered(rec.message as WireMessage);
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
        if (this.#skipTs == null) this.#onAssistantEvent(rec.assistantMessageEvent, rec.usage?.output);
        break;
      case 'message_end':
        this.#rawAdd(rec.message as WireMessage);
        this.#onMessageEnd(rec.message as WireMessage);
        break;

      case 'tool_execution_start': {
        const t = this.#findTool(rec.toolCallId);
        if (t) { t.status = 'running'; t.args = rec.args ?? t.args; if (!this.#replaying) t.startedAt = Date.now(); }
        if (!this.#replaying) this.sa.started(rec.toolCallId, rec.toolName, rec.args);
        if (/^(bg_(run|delegate)|codemode)/.test(rec.toolName) && !this.#replaying) {
          this.#launching.set(rec.toolCallId, rec.toolName);
          setTimeout(() => this.#launching.delete(rec.toolCallId), 60_000);
        }
        break;
      }
      case 'tool_execution_update': {
        const t = this.#findTool(rec.toolCallId);
        if (t) t.output = textOf(rec.partialResult?.content);
        this.sa.updated(rec.toolCallId, rec.toolName, rec.partialResult?.details);
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
        this.sa.ended(rec.toolCallId, rec.toolName, rec.result?.details, !!rec.isError);
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
        if (rec.errorMessage) this.notice('error', t('chatmsg.compactFailed', { err: rec.errorMessage }));
        else if (!rec.aborted && rec.result) void this.load();
        break;
      case 'auto_retry_start':
        this.retry = `${rec.attempt}/${rec.maxAttempts}`;
        // the reply that failed already shows the error in its own block: say only that it is tried again (the strip
        // and the spinner line count the tries), not the same error a second time
        if (!this.long && !this.#shownError(rec.errorMessage)) this.notice('warn', t('chatmsg.retrying', { n: rec.attempt, max: rec.maxAttempts, err: rec.errorMessage }));
        break;
      case 'auto_retry_end':
        this.retry = null;
        // "Retry cancelled": stopped on purpose (the person, or the server waiting for a plan to reset), not a failure
        if (!rec.success && !this.long && !/retry cancelled/i.test(rec.finalError ?? '')) this.notice('error', this.#shownError(rec.finalError) ? t('chatmsg.retryFailedShort') : t('chatmsg.retryFailed', { err: rec.finalError ?? '' }));
        break;

      case 'extension_ui_request':
        this.#onExtensionUi(rec as ExtensionRequest);
        break;
    }
  }

  #onAssistantEvent(ev: any, outputTokens?: number) {
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
        live.speed = this.#timer.snapshot(now, outputTokens ?? ev.partial?.usage?.output);
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
        if (m.errorMessage) live.errorAt = Date.now(); // the block shakes when the error arrives, however long the answer ran
        live.ts = m.timestamp ?? live.ts;
        const at = this.items.indexOf(live);
        if (at > 0 && repeatsFailure(this.items[at - 1], live)) this.items.splice(at - 1, 1);
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
          if (!this.#replaying) this.#taskArrived(x.id, x.status);
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
        const t = taskItem(m) ?? subagentItem(m);
        if (t?.kind === 'subagent' && this.items.some((i) => i.kind === 'subagent' && i.notice.runIds.some((r) => t.notice.runIds.includes(r)))) break;
        if (t) {
          this.items.push(t);
          if (!this.#replaying && t.kind === 'task') this.#taskArrived(t.id, t.status);
          if (!this.#replaying && t.kind === 'subagent') this.sa.arrived(t.notice, t.key);
        } else if (m.customType === BATON_NOTE) this.#batonArrived((m.details ?? {}) as BatonNote, true, m.timestamp);
        else if (m.display) this.items.push({ kind: 'summary', key: nextKey(), label: customLabel(m.customType), text: textOf(m.content), ts: m.timestamp });
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
        // extensions' own notices ("… loaded"); errors still show even with the banners turned off
        if (settings.extNotices || r.notifyType === 'error') this.toast(stripAnsi(r.message), r.notifyType ?? 'info');
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
        if (r.statusKey === 'oven-limits') {
          void limits.refresh();
          break;
        }
        if (r.statusKey === 'oven-permission') {
          // The server's choice is the truth (shared by every device). Only when nothing was ever chosen there
          // is the choice this browser kept from before brought over.
          this.permission = r.statusText?.startsWith('manual') ? 'manual' : 'auto';
          if (r.statusText?.endsWith('-default') && storedPermission() !== this.permission) this.setPermission(storedPermission());
          break;
        }
        const text = r.statusText ? stripAnsi(r.statusText).trim() : '';
        if (text && text !== '−') this.statuses[r.statusKey] = text;
        else delete this.statuses[r.statusKey];
        // an extension that shows a status has often just changed something (a long-context one swaps the model for a
        // copy with a larger window, after this page has read it): read the model and context size again
        this.#rereadSoon();
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
