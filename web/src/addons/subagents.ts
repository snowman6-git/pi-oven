// Add-on for `pi-subagents`: the model hands work to child agents with its `subagent` tool. A run is one call:
// one agent, or several (in parallel or one after another). Here every source of news about a run is turned into
// the same shape, so the chips, the tray and the notice in the conversation draw from one place:
// - the call's arguments (who was asked, before anything else is known),
// - the call's live updates and result (`details.progress` / `details.results`; a background launch says where its
//   runner keeps `status.json`),
// - that status file, read through the server while a background run goes on,
// - the notice pi puts in the conversation when a background run ends (`subagent-notify`).

export const TOOL = 'subagent';
export const NOTICE = 'subagent-notify';
export const COMMAND = 'subagents-doctor';

export type StepStatus = 'pending' | 'running' | 'done' | 'failed';
export type RunState = 'running' | 'done' | 'failed';
export type RunShape = 'one' | 'chain' | 'parallel';

export interface Step {
  agent: string;
  status: StepStatus;
  /** What it was asked to do, one line. */
  task?: string;
  /** The tool it is using now, and on what. */
  tool?: string;
  toolArgs?: string;
  /** `tool` is the last one it used (it is between tools: thinking, or waiting on the model), not one under way. */
  recent?: boolean;
  turns?: number;
  tools?: number;
  /** Tokens in its context now. */
  context?: number;
  durationMs?: number;
}

export interface Run {
  /** The tool call that started it (the row in the conversation). */
  toolId: string;
  /** pi-subagents' own id for a background run (what its notice names). */
  runId?: string;
  shape: RunShape;
  background: boolean;
  /** Where its runner keeps `status.json` (background only). */
  asyncDir?: string;
  steps: Step[];
  state: RunState;
  startedAt: number;
  endedAt?: number;
}

/** A finished background run as pi reported it in the conversation. */
export interface Notice {
  runIds: string[];
  agents: string[];
  status: 'completed' | 'failed' | 'stopped' | 'paused';
  durationMs?: number;
  /** What came back, without the instructions meant for the model. */
  text: string;
}

const str = (v: unknown, n = 300): string | undefined => {
  if (typeof v !== 'string' || !v.trim()) return undefined;
  const s = v.replace(/\s+/g, ' ').trim();
  return s.length > n ? s.slice(0, n) + '…' : s;
};
const num = (v: unknown) => (typeof v === 'number' && Number.isFinite(v) ? v : undefined);

function stepStatus(s: unknown): StepStatus {
  switch (s) {
    case 'running': return 'running';
    case 'complete': case 'completed': return 'done';
    case 'failed': case 'stopped': case 'rejected': case 'partial': return 'failed';
    default: return 'pending';
  }
}

/** A workflow script can run its children side by side (`runs.all`) or one after another: side by side when two
    of them are going at once or started together. */
function shapeOf(mode: unknown, n: number, starts: (number | undefined)[] = [], going = 0): RunShape {
  if (mode === 'parallel') return 'parallel';
  if (n <= 1) return 'one';
  if (mode === 'workflow') {
    const t = starts.filter((x): x is number => x != null).sort((a, b) => a - b);
    if (going > 1 || (t.length > 1 && t[1] - t[0] < 3000)) return 'parallel';
  }
  return 'chain';
}

/** What an agent was asked, in words: its session's name ("reviewer: 읽기 전용 검토…") without the agent's name,
    else the description, unless the package hid it. */
function taskOf(s: any): string | undefined {
  const name = typeof s?.sessionName === 'string' ? s.sessionName.replace(/^[\w.-]+:\s*/, '') : undefined;
  const desc = typeof s?.description === 'string' && !/^\[[^\]]*redacted\]$/i.test(s.description.trim()) ? s.description : undefined;
  return str(name) ?? str(desc) ?? str(s?.label, 80);
}

/** Management calls (`status`, `list`, `guide` …) start nothing. */
export function startsRun(args: any): boolean {
  return !!args && typeof args === 'object' && !args.action && (typeof args.agent === 'string' || !!args.workflow);
}

/** A new run from the call's arguments: the agent it names, or one placeholder step for a workflow. */
export function fromArgs(toolId: string, args: any, now: number): Run | null {
  if (!startsRun(args)) return null;
  const agent = str(args.agent, 80) ?? 'workflow';
  return {
    toolId,
    shape: 'one',
    background: args.async !== false, // pi-subagents runs in the background unless told not to
    steps: [{ agent, status: 'running', task: str(args.task) }],
    state: 'running',
    startedAt: now,
  };
}

/** Steps from a live update or a result (`details.progress`, else `details.results`). */
function stepsFromDetails(d: any): Step[] | null {
  const progress = Array.isArray(d?.progress) ? d.progress : null;
  const results = Array.isArray(d?.results) ? d.results : null;
  const list = progress?.length ? progress : results;
  if (!list?.length) return null;
  const out: Step[] = [];
  for (const p of list.slice(0, 64)) {
    const agent = str(p?.agent, 80);
    if (!agent) continue;
    // a result row has no status of its own: its exit code says how it went
    const status = p?.status != null ? stepStatus(p.status) : p?.exitCode === 0 ? 'done' : p?.exitCode != null ? 'failed' : 'pending';
    out.push({ agent, status, task: str(p?.task), tool: str(p?.currentTool, 80), toolArgs: str(p?.currentToolArgs, 200), durationMs: num(p?.durationMs) });
  }
  return out.length ? out : null;
}

/** Fold a live update (`final` false) or the call's result (`final` true) into the run. */
export function applyDetails(run: Run, d: any, final: boolean, isError: boolean, now: number): Run {
  const next: Run = { ...run };
  const steps = stepsFromDetails(d);
  if (steps) next.steps = steps;
  next.shape = shapeOf(d?.mode, next.steps.length, [], next.steps.filter((s) => s.status === 'running').length);
  if (typeof d?.asyncId === 'string') next.runId = d.asyncId;
  else if (typeof d?.runId === 'string' && !next.runId) next.runId = d.runId;
  if (typeof d?.asyncDir === 'string') next.asyncDir = d.asyncDir;
  if (d?.background === true || typeof d?.asyncDir === 'string') next.background = true;
  if (!final) return next;
  if (isError) {
    next.state = 'failed';
    next.endedAt = now;
  } else if (next.asyncDir) {
    // launched: it goes on in its runner, and the status file takes over
    next.state = 'running';
  } else {
    next.background = false;
    next.state = next.steps.some((s) => s.status === 'failed') ? 'failed' : 'done';
    next.steps = next.steps.map((s) => (s.status === 'running' || s.status === 'pending' ? { ...s, status: next.state === 'done' ? 'done' : s.status } : s));
    next.endedAt = now;
  }
  return next;
}

/** Fold what the server read from the runner's `status.json` into the run. */
export function applyStatus(run: Run, st: any, now: number): Run {
  if (!st || typeof st !== 'object') return run;
  const next: Run = { ...run };
  if (typeof st.runId === 'string') next.runId = st.runId;
  const steps: Step[] = [];
  const starts: (number | undefined)[] = [];
  for (const s of Array.isArray(st.steps) ? st.steps.slice(0, 64) : []) {
    const agent = str(s?.agent, 80);
    if (!agent) continue;
    const now = str(s?.currentTool, 80);
    const last = str(s?.recentTool, 80);
    starts.push(num(s?.startedAt));
    steps.push({
      agent, status: stepStatus(s?.status), task: taskOf(s),
      tool: now ?? last, toolArgs: now ? str(s?.currentToolArgs, 200) : str(s?.recentToolArgs, 200), recent: !now && !!last,
      turns: num(s?.turnCount), tools: num(s?.toolCount), context: num(s?.context), durationMs: num(s?.durationMs),
    });
  }
  if (steps.length) next.steps = steps;
  next.shape = shapeOf(st.mode, next.steps.length, starts, steps.filter((s) => s.status === 'running').length);
  if (typeof st.startedAt === 'number') next.startedAt = st.startedAt;
  switch (st.state) {
    case 'complete':
      next.state = 'done';
      break;
    case 'failed': case 'stopped': case 'rejected': case 'partial':
      next.state = 'failed';
      break;
    default:
      next.state = 'running';
  }
  if (next.state !== 'running') next.endedAt = num(st.endedAt) ?? now;
  return next;
}

/** pi's notice that a background run has ended. `null` if the message is not one. */
export function noticeFrom(m: { customType?: string; content?: unknown; details?: any }): Notice | null {
  if (m.customType !== NOTICE) return null;
  const runs: any[] = Array.isArray(m.details?.runs) ? m.details.runs : [];
  const text = typeof m.content === 'string' ? m.content : Array.isArray(m.content) ? m.content.map((p: any) => (p?.type === 'text' ? p.text : '')).join('') : '';
  // what follows "Parent action:" is addressed to the model, not to the person reading; the first line
  // ("Background task completed: **reviewer**") says what the card's own head already says
  const shown = text
    .split(/\n\s*Parent action:/)[0]
    .replace(/^(?:Background tasks? completed[^\n]*|(?:Background|Detached foreground) task \w+:[^\n]*)\n?/, '')
    .trim();
  const status = runs.some((r) => r?.status === 'failed') ? 'failed' : runs.some((r) => r?.status === 'stopped') ? 'stopped' : runs.some((r) => r?.status === 'paused') ? 'paused' : 'completed';
  const durations = runs.map((r) => num(r?.durationMs)).filter((x): x is number => x != null);
  return {
    runIds: runs.map((r) => (typeof r?.runId === 'string' ? r.runId : '')).filter(Boolean),
    agents: runs.map((r) => str(r?.agent, 80)).filter((x): x is string => !!x),
    status,
    durationMs: durations.length ? Math.max(...durations) : undefined,
    text: shown,
  };
}

/** The name a run goes by in a chip: the agent doing the work now, `reviewer ×3` for the same one in parallel. */
export function runName(r: Run): string {
  if (r.shape === 'parallel') {
    const names = new Set(r.steps.map((s) => s.agent));
    return names.size === 1 ? `${r.steps[0].agent} ×${r.steps.length}` : r.steps.map((s) => s.agent).join(' · ');
  }
  const now = r.steps.find((s) => s.status === 'running') ?? r.steps.find((s) => s.status === 'pending') ?? r.steps[r.steps.length - 1];
  return now?.agent ?? 'subagent';
}

/** "2/3": a chain's step under way, of how many; side by side, how many have finished. Nothing for one agent. */
export function runCount(r: Run): string {
  if (r.shape === 'parallel') return `${r.steps.filter((s) => s.status === 'done' || s.status === 'failed').length}/${r.steps.length}`;
  if (r.shape !== 'chain') return '';
  const i = r.steps.findIndex((s) => s.status === 'running' || s.status === 'pending');
  return `${i < 0 ? r.steps.length : i + 1}/${r.steps.length}`;
}

/** The notice's own line: agents, `reviewer ×3` when one agent ran several times. */
export function noticeName(n: Notice): string {
  if (!n.agents.length) return 'subagent';
  const names = new Set(n.agents);
  return names.size === 1 && n.agents.length > 1 ? `${n.agents[0]} ×${n.agents.length}` : [...names].join(' · ');
}
