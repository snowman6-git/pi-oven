// Add-on for `@juicesharp/rpiv-todo`: the model keeps a task list with its `todo` tool, and that package draws it
// in the terminal. Here the list is read back out of the conversation (every call's `details` holds the whole
// list) and, for calls that never show up as a tool result, from the entries the package saves.

export type TaskStatus = 'pending' | 'in_progress' | 'completed' | 'deleted';

export interface Task {
  id: number;
  subject: string;
  description?: string;
  /** The present-continuous label shown while the task is in progress ("라우트 연결하는 중"). */
  activeForm?: string;
  status: TaskStatus;
  blockedBy?: number[];
  owner?: string;
}

export const TOOL = 'todo';
export const STATE_ENTRY = 'rpiv-todo-state';
export const COMMAND = 'todos';

const STATUSES = new Set(['pending', 'in_progress', 'completed', 'deleted']);

/** The tasks inside a `details` snapshot, or null if it is not one. Anything odd in a task drops that task, not the list. */
export function fromDetails(d: unknown): Task[] | null {
  const list = (d as { tasks?: unknown } | null)?.tasks;
  if (!Array.isArray(list)) return null;
  const out: Task[] = [];
  for (const t of list) {
    if (!t || typeof t !== 'object') continue;
    const { id, subject, status } = t as Record<string, unknown>;
    if (typeof id !== 'number' || typeof subject !== 'string' || typeof status !== 'string' || !STATUSES.has(status)) continue;
    const x = t as Record<string, unknown>;
    out.push({
      id,
      subject,
      status: status as TaskStatus,
      description: typeof x.description === 'string' ? x.description : undefined,
      activeForm: typeof x.activeForm === 'string' ? x.activeForm : undefined,
      blockedBy: Array.isArray(x.blockedBy) ? x.blockedBy.filter((n): n is number => typeof n === 'number') : undefined,
      owner: typeof x.owner === 'string' ? x.owner : undefined,
    });
  }
  return out;
}

/** The newest successful `todo` call in the conversation holds the list as it stood then. */
export function latestFromItems(items: { kind: string; blocks?: any[] }[]): Task[] | null {
  for (let i = items.length - 1; i >= 0; i--) {
    const it = items[i];
    if (it.kind !== 'assistant') continue;
    for (let j = (it.blocks?.length ?? 0) - 1; j >= 0; j--) {
      const b = it.blocks![j];
      if (b?.type === 'tool' && b.name === TOOL && b.status === 'ok') {
        const tasks = fromDetails(b.details);
        if (tasks) return tasks;
      }
    }
  }
  return null;
}

export interface Summary {
  /** Tasks that count (not deleted), in id order. */
  tasks: Task[];
  total: number;
  done: number;
  /** What is being worked on, else what is next and free to start. */
  current: Task | null;
  allDone: boolean;
}

export function summarize(all: Task[]): Summary {
  const tasks = all.filter((t) => t.status !== 'deleted').sort((a, b) => a.id - b.id);
  const done = tasks.filter((t) => t.status === 'completed').length;
  const doing = tasks.find((t) => t.status === 'in_progress');
  const next = tasks.find((t) => t.status === 'pending' && unfinished(t, tasks).length === 0);
  return { tasks, total: tasks.length, done, current: doing ?? next ?? null, allDone: tasks.length > 0 && done === tasks.length };
}

/** The ids this task still waits for. */
export function unfinished(t: Task, tasks: Task[]): number[] {
  return (t.blockedBy ?? []).filter((id) => {
    const dep = tasks.find((x) => x.id === id);
    return dep && dep.status !== 'completed' && dep.status !== 'deleted';
  });
}

/** One line for the folded tray: what is happening, in words. */
export function headline(s: Summary): string {
  if (s.total === 0) return '할 일이 없습니다';
  if (s.allDone) return '모두 끝났어요';
  const c = s.current;
  if (!c) return '막힌 일만 남았어요';
  return c.status === 'in_progress' ? c.activeForm || c.subject : `다음: ${c.subject}`;
}
