import type { BgTask, DirListing, RuntimeInfo, SessionSummary } from './protocol';
import type { Entry } from './tree';

import { serverUrl } from './settings.svelte';

const TOKEN_KEY = 'pi-oven.token';

export class AuthError extends Error {
  constructor(readonly why: 'wrong' | 'locked' = 'wrong') {
    super(why);
  }
}
/** A refused request, with the status and whatever the server said (a save that lost a race says which revision is on disk). */
export class HttpError extends Error {
  constructor(message: string, readonly status: number, readonly body: any = null) {
    super(message);
  }
}

export const auth = {
  get token(): string | null {
    try { return localStorage.getItem(TOKEN_KEY); } catch { return null; }
  },
  set token(v: string | null) {
    try { v ? localStorage.setItem(TOKEN_KEY, v) : localStorage.removeItem(TOKEN_KEY); } catch { /* private mode */ }
    // ask the browser not to evict the saved login when it is short of space
    if (v) void navigator.storage?.persist?.().catch(() => {});
  },
  /** Accept `#token=…` from the printed URL, store it, and clean the address bar. */
  captureFromHash() {
    const m = location.hash.match(/token=([0-9a-zA-Z_-]+)/);
    if (m) {
      this.token = m[1];
      history.replaceState(null, '', location.pathname + location.search);
    }
  },
};

async function call<T>(path: string, init: RequestInit = {}): Promise<T> {
  const headers = new Headers(init.headers);
  if (auth.token) headers.set('Authorization', `Bearer ${auth.token}`);
  if (init.body) headers.set('Content-Type', 'application/json');
  let res: Response;
  try {
    res = await fetch(serverUrl.value + path, { ...init, headers });
  } catch {
    throw new Error('network');
  }
  if (res.status === 401) throw new AuthError();
  if (res.status === 429) throw new AuthError('locked');
  if (!res.ok) {
    const body = await res.json().catch(() => null);
    throw new HttpError(body?.error ?? `${res.status} ${res.statusText}`, res.status, body);
  }
  return res.status === 204 ? (undefined as T) : res.json();
}

export interface SessionTree { path: string; cwd: string; leafId: string | null; entries: Entry[]; labels: Record<string, string> }
export interface Branched { path: string; id: string; cwd: string; entries: number; text: string | null }
export interface GitFile { path: string; status: string; staged: boolean; unstaged: boolean; added: number | null; removed: number | null }
export interface GitCommit { hash: string; subject: string; when: number }
export interface GitOverview { repo: boolean; branch: string | null; root: string | null; files: GitFile[]; truncated: boolean; commits: GitCommit[] }

async function callText(path: string): Promise<string> {
  const headers = new Headers();
  if (auth.token) headers.set('Authorization', `Bearer ${auth.token}`);
  let res: Response;
  try { res = await fetch(serverUrl.value + path, { headers }); } catch { throw new Error('network'); }
  if (res.status === 401) throw new AuthError();
  if (!res.ok) throw new Error(`${res.status} ${res.statusText}`);
  return res.text();
}

export interface FileEntry { name: string; kind: 'dir' | 'file'; size: number; mtime: number; link: boolean; blocked: boolean }
export interface FileListing { path: string; entries: FileEntry[]; truncated: boolean }
export interface FileText { path: string; content: string; rev: string; mtime: number; size: number; eol: 'lf' | 'crlf'; bom: boolean }
export interface FileStat { rev: string; mtime: number; size: number }

export interface ModelProvider {
  name: string;
  baseUrl?: string;
  api?: string;
  key: { kind: 'none' | 'literal' | 'env' | 'command'; hint?: string };
  models: { id: string; name?: string; reasoning: boolean; image: boolean; contextWindow?: number; maxTokens?: number }[];
}
export interface ModelsConfig { path: string; providers: ModelProvider[]; apis: string[] }
export interface ModelForm {
  provider: string;
  baseUrl: string;
  api: string;
  /** Left out: keep the key that is there. Empty: take it away. */
  apiKey?: string;
  model?: { id: string; name?: string; reasoning: boolean; image: boolean; contextWindow?: number; maxTokens?: number };
}

export const api = {
  modelsConfig: () => call<ModelsConfig>('/api/models/config'),
  saveModel: (body: ModelForm) => call<void>('/api/models/config', { method: 'POST', body: JSON.stringify(body) }),
  removeModel: (provider: string, model?: string) =>
    call<void>(`/api/models/config?provider=${encodeURIComponent(provider)}${model ? `&model=${encodeURIComponent(model)}` : ''}`, { method: 'DELETE' }),
  probeModels: (body: { baseUrl: string; apiKey?: string; provider?: string }) =>
    call<{ ids: string[] }>('/api/models/probe', { method: 'POST', body: JSON.stringify(body) }),
  sessionTree: (path: string) => call<SessionTree>(`/api/sessions/tree?path=${encodeURIComponent(path)}`),
  /** A new session file holding only the way from the start to `leaf` (null: nothing before the message). */
  branchSession: (path: string, leaf: string | null, textOf?: string) =>
    call<Branched>('/api/sessions/branch', { method: 'POST', body: JSON.stringify({ path, leaf, textOf }) }),
  git: (id: string) => call<GitOverview>(`/api/runtimes/${id}/git`),
  gitStage: (id: string, paths: string[], stage: boolean) =>
    call<void>(`/api/runtimes/${id}/git/stage`, { method: 'POST', body: JSON.stringify({ paths, stage }) }),
  gitCommit: (id: string, message: string) =>
    call<{ hash: string }>(`/api/runtimes/${id}/git/commit`, { method: 'POST', body: JSON.stringify({ message }) }),
  /** The newest saved state of an extension (`appendEntry`) on the branch the session stands on, or null. */
  sessionState: (path: string, type: string) => call<{ data: unknown }>(`/api/sessions/state?path=${encodeURIComponent(path)}&type=${encodeURIComponent(type)}`),
  files: (id: string, path = '') => call<FileListing>(`/api/runtimes/${id}/files?path=${encodeURIComponent(path)}`),
  readFile: (id: string, path: string) => call<FileText>(`/api/runtimes/${id}/files/read?path=${encodeURIComponent(path)}`),
  writeFile: (id: string, body: { path: string; content: string; rev?: string | null; overwrite?: boolean; eol?: string; bom?: boolean }) =>
    call<{ rev: string; mtime: number; size: number }>(`/api/runtimes/${id}/files/write`, { method: 'PUT', body: JSON.stringify(body) }),
  createFile: (id: string, path: string, kind: 'file' | 'dir') =>
    call<void>(`/api/runtimes/${id}/files/create`, { method: 'POST', body: JSON.stringify({ path, kind }) }),
  renameFile: (id: string, from: string, to: string) =>
    call<void>(`/api/runtimes/${id}/files/rename`, { method: 'POST', body: JSON.stringify({ from, to }) }),
  deleteFile: (id: string, path: string) => call<void>(`/api/runtimes/${id}/files?path=${encodeURIComponent(path)}`, { method: 'DELETE' }),
  statFiles: (id: string, paths: string[]) =>
    call<Record<string, FileStat | null>>(`/api/runtimes/${id}/files/stat`, { method: 'POST', body: JSON.stringify({ paths }) }),
  gitDiff: (id: string, path: string) => callText(`/api/runtimes/${id}/git/diff?path=${encodeURIComponent(path)}`),
  info: () => call<{ version: string; home: string; sessionsDir: string }>('/api/info'),
  sessions: () => call<SessionSummary[]>('/api/sessions'),
  /** Name a session that is not open (an empty name clears it). An open one is renamed through its own pi. */
  renameSession: (path: string, name: string) => call<void>('/api/sessions/rename', { method: 'POST', body: JSON.stringify({ path, name }) }),
  deleteSession: (path: string) => call<void>(`/api/sessions?path=${encodeURIComponent(path)}`, { method: 'DELETE' }),
  runtimes: () => call<RuntimeInfo[]>('/api/runtimes'),
  openRuntime: (body: { cwd: string; sessionPath?: string }) =>
    call<RuntimeInfo>('/api/runtimes', { method: 'POST', body: JSON.stringify(body) }),
  restartRuntime: (id: string, sessionPath?: string) =>
    call<RuntimeInfo>(`/api/runtimes/${id}/restart`, { method: 'POST', body: JSON.stringify({ sessionPath }) }),
  tasks: (id: string) => call<BgTask[]>(`/api/runtimes/${id}/tasks`),
  async taskOutput(id: string, task: string, tail = 24 * 1024): Promise<{ text: string; total: number }> {
    const headers = new Headers();
    if (auth.token) headers.set('Authorization', `Bearer ${auth.token}`);
    const res = await fetch(`${serverUrl.value}/api/runtimes/${id}/tasks/${task}/output?tail=${tail}`, { headers });
    if (res.status === 404) return { text: '', total: 0 };       // nothing written yet
    if (!res.ok) throw new Error(`${res.status} ${res.statusText}`);
    return { text: await res.text(), total: Number(res.headers.get('x-total-bytes') ?? 0) };
  },
  closeRuntime: (id: string) => call<void>(`/api/runtimes/${id}`, { method: 'DELETE' }),
  browse: (path?: string, hidden = false) =>
    call<DirListing>(`/api/fs?${new URLSearchParams({ ...(path ? { path } : {}), hidden: String(hidden) })}`),
  wsUrl(id: string) {
    const base = new URL(serverUrl.value || location.origin);
    const proto = base.protocol === 'https:' ? 'wss' : 'ws';
    const q = auth.token ? `?token=${encodeURIComponent(auth.token)}` : '';
    return `${proto}://${base.host}/api/runtimes/${id}/ws${q}`;
  },
};
