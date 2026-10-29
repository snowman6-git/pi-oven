// Which files a session touched, from pi's own `edit` and `write` tool calls. This is the session's
// account of itself; git (see the server) is the account of what really changed, `bash` included.

import type { Item, ToolBlock } from './chat.svelte';

export interface Touch {
  kind: 'edit' | 'write';
  /** pi's own diff for an edit ("+ 12 text" / "- 12 text"), or the full content for a write. */
  diff?: string;
  content?: string;
}

export interface FileChange {
  path: string;
  touches: Touch[];
  added: number;
  removed: number;
  edits: number;
  writes: number;
  /** Order of the latest touch among all touches of the session: the biggest is the file touched last. */
  lastSeq: number;
}

/** Lines of a pi diff that are additions / removals. */
export function countDiff(diff: string): { added: number; removed: number } {
  let added = 0;
  let removed = 0;
  for (const line of diff.split('\n')) {
    if (line.startsWith('+')) added++;
    else if (line.startsWith('-')) removed++;
  }
  return { added, removed };
}

const lines = (s: string) => (s === '' ? 0 : s.endsWith('\n') ? s.split('\n').length - 1 : s.split('\n').length);

/** Only calls that succeeded count: a refused or failed edit changed nothing. */
export function sessionChanges(items: Item[]): FileChange[] {
  const byPath = new Map<string, FileChange>();
  let seq = 0;
  for (const item of items) {
    if (item.kind !== 'assistant') continue;
    for (const block of item.blocks) {
      if (!block || block.type !== 'tool') continue;
      const t = block as ToolBlock;
      if (t.status !== 'ok' || (t.name !== 'edit' && t.name !== 'write')) continue;
      const path = typeof t.args?.path === 'string' ? t.args.path : '';
      if (!path) continue;
      let f = byPath.get(path);
      if (!f) byPath.set(path, (f = { path, touches: [], added: 0, removed: 0, edits: 0, writes: 0, lastSeq: 0 }));
      f.lastSeq = ++seq;
      if (t.name === 'edit') {
        const diff = typeof t.details?.diff === 'string' ? (t.details.diff as string) : '';
        const c = countDiff(diff);
        f.touches.push({ kind: 'edit', diff });
        f.added += c.added;
        f.removed += c.removed;
        f.edits++;
      } else {
        const content = typeof t.args?.content === 'string' ? (t.args.content as string) : '';
        f.touches.push({ kind: 'write', content });
        f.added += lines(content);
        f.writes++;
      }
    }
  }
  return [...byPath.values()];
}

/** A path relative to the session's folder when it is inside it, for display. */
export function shortPath(path: string, cwd: string): string {
  const base = cwd.endsWith('/') ? cwd : cwd + '/';
  return cwd && path.startsWith(base) ? path.slice(base.length) : path;
}

/** The content of a write as a pi-style diff, so one viewer shows both. Capped; the rest is noted. */
export function contentAsDiff(content: string, max = 400): string {
  const all = content.split('\n');
  if (all[all.length - 1] === '') all.pop();
  const shown = all.slice(0, max).map((l, i) => `+ ${i + 1} ${l}`);
  if (all.length > max) shown.push(`     ... ${all.length - max} more lines`);
  return shown.join('\n');
}

/** Edits and writes pi has started and not finished: the file is being changed right now. */
export function runningEdits(items: Item[]): string[] {
  const out = new Set<string>();
  for (const item of items) {
    if (item.kind !== 'assistant') continue;
    for (const block of item.blocks) {
      if (!block || block.type !== 'tool') continue;
      const t = block as ToolBlock;
      if ((t.name === 'edit' || t.name === 'write') && (t.status === 'running' || t.status === 'pending') && typeof t.args?.path === 'string') out.add(t.args.path);
    }
  }
  return [...out];
}
