// The subagent runs of one conversation, as this page follows them (pi-subagents; the shapes are in addons/subagents).
//
// A run starts with its `subagent` tool call. One in the foreground reports through the call's live updates and
// ends with the call. One in the background (the package's default) is launched by the call and goes on in a
// runner of its own: its `status.json` is read through the server every 2 s, and pi puts a notice in the
// conversation when it ends.
//
// What shows: a chip (or the tray, Settings → Theme) for every run going on, and for one that has just ended until it
// is handed to the conversation: the chip flies to the notice (background) or back to its tool row (foreground),
// and the notice stays hidden until it lands. The flights themselves are BgFlight's.

import { api } from './api';
import {
  NOTICE, TOOL, applyDetails, applyStatus, fromArgs, noticeFrom, type Notice, type Run, type RunState,
} from '../addons/subagents';
import type { Item } from './transcript';

export interface SubagentHost {
  /** pi-subagents is installed in this pi (its commands are there): otherwise nothing here runs or shows. */
  enabled(): boolean;
  sessionFile(): string | undefined;
  /** The conversation is on screen and the page is in view: only then is a chip flown. */
  visible(): boolean;
  /** Put a notice found in the session file into the conversation (unless it is there already). Its item key, or null. */
  addNotice(rec: any): number | null;
}

const POLL_MS = 2000;
/** A finished run waits this long for its notice before its chip goes by itself. */
const NOTICE_WAIT_MS = 15 * 60_000;
/** A foreground run's chip stays this long after it ends (its result is right there in its tool row). */
const FOREGROUND_LINGER_MS = 1200;

export class Subagents {
  runs = $state<Run[]>([]);
  /** Ended, and the chip is still up (until it is handed over or goes). Tool ids. */
  fresh = $state<string[]>([]);
  /** Just started: hidden until the ember from its tool row lands on it. */
  landing = $state<string[]>([]);
  fading = $state<string[]>([]);
  launches = $state<{ n: number; toolId: string }[]>([]);
  events = $state<{ n: number; id: string; state: RunState }[]>([]);
  /** A chip to fly into the conversation: to the notice with `key`, or (no key) back to its tool row. */
  handoffs = $state<{ n: number; id: string; key: number | null }[]>([]);
  /** Notice items kept hidden until their chip lands. */
  hold = $state<number[]>([]);
  /** The run whose sheet is open (its tool id; `*` for all of them), or none. */
  open = $state<string | null>(null);

  #seq = 0;
  #pollTimer: ReturnType<typeof setTimeout> | undefined;
  #tailTimer: ReturnType<typeof setTimeout> | undefined;
  /** Tool ids of ended background runs whose notice has not come. */
  #awaiting = new Set<string>();
  #disposed = false;

  constructor(private host: SubagentHost) {}

  /** Chips showing now: going, or just ended. Oldest first. */
  get chips(): Run[] {
    if (!this.host.enabled()) return [];
    return this.runs.filter((r) => r.state === 'running' || this.fresh.includes(r.toolId)).sort((a, b) => a.startedAt - b.startedAt);
  }

  #set(run: Run) {
    const i = this.runs.findIndex((r) => r.toolId === run.toolId);
    if (i < 0) this.runs = [...this.runs.slice(-40), run];
    else this.runs = this.runs.map((r, j) => (j === i ? run : r));
  }
  #get(toolId: string) {
    return this.runs.find((r) => r.toolId === toolId);
  }

  // ── what pi says ───────────────────────────────────────────────────────────

  started(toolId: string, name: string, args: any) {
    if (name !== TOOL || this.#get(toolId) || !this.host.enabled()) return;
    const run = fromArgs(toolId, args, Date.now());
    if (!run) return;
    this.#set(run);
    this.landing = [...this.landing, toolId];
    setTimeout(() => (this.landing = this.landing.filter((x) => x !== toolId)), 20_000); // never stuck hidden
    this.launches = [...this.launches.slice(-30), { n: ++this.#seq, toolId }];
  }

  updated(toolId: string, name: string, details: any) {
    if (name !== TOOL || !details || !this.host.enabled()) return;
    // a run already under way when this page (re)loaded: picked up quietly from its first update
    const run = this.#get(toolId) ?? fromArgs(toolId, { agent: 'subagent', async: false }, Date.now());
    if (run) this.#set(applyDetails(run, details, false, false, Date.now()));
  }

  ended(toolId: string, name: string, details: any, isError: boolean) {
    const run = name === TOOL ? this.#get(toolId) : undefined;
    if (!run) return;
    const next = applyDetails(run, details, true, isError, Date.now());
    this.#set(next);
    if (next.state === 'running') this.#poll();
    else this.#ended(next);
  }

  /** A notice reached the conversation (live, or found in the file) as the item with `key`. */
  arrived(notice: Notice, key: number) {
    const run =
      this.runs.find((r) => r.runId && notice.runIds.includes(r.runId)) ??
      // no id to match (an older package): the oldest background run that ended or is going with one of its agents
      this.runs.filter((r) => r.background && (this.#awaiting.has(r.toolId) || r.state === 'running') && r.steps.some((s) => notice.agents.includes(s.agent))).sort((a, b) => a.startedAt - b.startedAt)[0];
    if (!run) return;
    if (run.state === 'running') {
      // the notice can beat the status file
      const ended: Run = { ...run, state: notice.status === 'completed' ? 'done' : 'failed', endedAt: Date.now() };
      this.#set(ended);
      this.#ended(ended, false);
    }
    this.#awaiting.delete(run.toolId);
    this.#handOver(run.toolId, key);
  }

  /** After the conversation was (re)loaded: background runs launched in it whose notice is not there are going on. */
  restore(items: Item[]) {
    if (!this.host.enabled()) return;
    const noticed = new Set(items.flatMap((i) => (i.kind === 'subagent' ? i.notice.runIds : [])));
    for (const it of items) {
      if (it.kind !== 'assistant') continue;
      for (const b of it.blocks) {
        if (b.type !== 'tool' || b.name !== TOOL || this.#get(b.id)) continue;
        const d = b.details;
        if (typeof d?.asyncDir !== 'string' || (typeof d?.asyncId === 'string' && noticed.has(d.asyncId))) continue;
        const base = fromArgs(b.id, b.args, Date.now());
        if (!base) continue;
        this.#set(applyDetails(base, d, true, false, Date.now()));
      }
    }
    // runs this page was already following whose notice came while it was away (a reconnect, a phone asleep): they
    // are over, and the notice is in the conversation already, so the chip just goes (no flight, no 15-minute wait)
    const status = new Map(items.flatMap((i) => (i.kind === 'subagent' ? i.notice.runIds.map((id) => [id, i.notice.status] as const) : [])));
    for (const r of this.runs) {
      const st = r.runId ? status.get(r.runId) : undefined;
      if (!st) continue;
      if (r.state === 'running') this.#set({ ...r, state: st === 'completed' ? 'done' : 'failed', endedAt: Date.now() });
      this.#awaiting.delete(r.toolId);
      this.dismiss(r.toolId);
    }
    this.#poll();
  }

  // ── following background runs ─────────────────────────────────────────────

  #poll() {
    if (this.#pollTimer || this.#disposed) return;
    this.#pollTimer = setTimeout(() => void this.#pollOnce(), 300);
  }

  async #pollOnce() {
    this.#pollTimer = undefined;
    const going = this.runs.filter((r) => r.state === 'running' && r.asyncDir);
    if (!going.length || this.#disposed) return;
    await Promise.all(
      going.map(async (r) => {
        try {
          const st = await api.subagentStatus(r.asyncDir!);
          const cur = this.#get(r.toolId);
          if (!cur || cur.state !== 'running') return;
          const next = applyStatus(cur, st, Date.now());
          this.#set(next);
          if (next.state !== 'running') this.#ended(next);
        } catch {
          /* not written yet, or being written: next round */
        }
      }),
    );
    if (this.runs.some((r) => r.state === 'running' && r.asyncDir)) this.#pollTimer = setTimeout(() => void this.#pollOnce(), document.hidden ? POLL_MS * 3 : POLL_MS);
  }

  #ended(run: Run, wait = true) {
    this.events = [...this.events.slice(-19), { n: ++this.#seq, id: run.toolId, state: run.state }];
    if (!this.fresh.includes(run.toolId)) this.fresh = [...this.fresh, run.toolId];
    if (!run.background) {
      // its result is in its own tool row: the chip goes back there
      setTimeout(() => this.#handOver(run.toolId, null), FOREGROUND_LINGER_MS);
      return;
    }
    if (!wait) return;
    this.#awaiting.add(run.toolId);
    this.#tail();
    setTimeout(() => {
      if (this.#awaiting.delete(run.toolId)) this.#expire(run.toolId);
    }, NOTICE_WAIT_MS);
  }

  /** A background run's notice may only land in the session file: read its tail while one is awaited. */
  #tail() {
    if (this.#tailTimer || this.#disposed) return;
    const round = async () => {
      this.#tailTimer = undefined;
      if (!this.#awaiting.size || this.#disposed) return;
      const file = this.host.sessionFile();
      if (file) {
        try {
          const { lines } = await api.tailSession(file, NOTICE);
          for (const line of lines) {
            let rec: any;
            try { rec = JSON.parse(line); } catch { continue; }
            if (rec.type !== 'custom_message' || rec.customType !== NOTICE) continue;
            const notice = noticeFrom(rec);
            if (!notice) continue;
            const key = this.host.addNotice(rec);
            if (key != null) this.arrived(notice, key);
          }
        } catch { /* next round */ }
      }
      if (this.#awaiting.size) this.#tailTimer = setTimeout(round, 1500);
    };
    this.#tailTimer = setTimeout(round, 800);
  }

  // ── chips leaving ─────────────────────────────────────────────────────────

  #handOver(toolId: string, key: number | null) {
    if (!this.fresh.includes(toolId)) return;
    if (!this.host.visible() || this.landing.includes(toolId)) {
      this.dismiss(toolId);
      return;
    }
    if (key != null) this.hold = [...this.hold, key];
    this.handoffs = [...this.handoffs.slice(-30), { n: ++this.#seq, id: toolId, key }];
    // never leave a notice hidden if nothing plays the flight
    setTimeout(() => this.landed(toolId, key), 8000);
  }

  /** The flight is over (or will not happen). */
  landed(toolId: string, key: number | null) {
    if (key != null) this.hold = this.hold.filter((k) => k !== key);
    this.dismiss(toolId);
  }

  dismiss(toolId: string) {
    this.fresh = this.fresh.filter((x) => x !== toolId);
  }

  #expire(toolId: string) {
    if (!this.fresh.includes(toolId)) return;
    this.fading = [...this.fading, toolId];
    setTimeout(() => {
      this.dismiss(toolId);
      this.fading = this.fading.filter((x) => x !== toolId);
    }, 340);
  }

  dispose() {
    this.#disposed = true;
    clearTimeout(this.#pollTimer);
    clearTimeout(this.#tailTimer);
  }
}
