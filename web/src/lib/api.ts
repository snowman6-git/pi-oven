import type { BgTask, DirListing, RuntimeInfo, SessionSummary } from './protocol';
import type { LimitWire } from './limits';
import type { Entry } from './tree';

import { serverUrl } from './settings.svelte';
import { currentLang } from './i18n';
import { tokenFromHash } from './util';

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

/** The 100h mode's switch. `available` is false when the server has no retry extension. */
export interface LongMode { enabled: boolean; available: boolean }
export interface ResumeSettings { enabled: boolean; text: string }

/** One saved login per server: switching to another address must not send it this one's token. The page's own
    server keeps the plain key, so a login saved before this still works. */
const tokenKey = () => (serverUrl.value ? `${TOKEN_KEY}@${serverUrl.value}` : TOKEN_KEY);

export const auth = {
  get token(): string | null {
    try { return localStorage.getItem(tokenKey()); } catch { return null; }
  },
  set token(v: string | null) {
    try { v ? localStorage.setItem(tokenKey(), v) : localStorage.removeItem(tokenKey()); } catch { /* private mode */ }
    // ask the browser not to evict the saved login when it is short of space
    if (v) void navigator.storage?.persist?.().catch(() => {});
  },
  /** Accept `#token=…` (the printed URL, or what the app's first screen sends: a token or a password, URL-encoded),
      store it, and clean the address bar. */
  captureFromHash() {
    const v = tokenFromHash(location.hash);
    if (v) {
      this.token = v;
      history.replaceState(null, '', location.pathname + location.search);
    }
  },
};

async function call<T>(path: string, init: RequestInit = {}): Promise<T> {
  const headers = new Headers(init.headers);
  if (auth.token) headers.set('Authorization', `Bearer ${auth.token}`);
  headers.set('X-Pi-Oven-Lang', currentLang()); // the server words its errors in the page's language
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

/** The newest release's server binary against the one running (first 12 hex digits of SHA-256). */
/** What the server's background check last found. */
export interface OvenStatus { auto: boolean; available: boolean; tag: string | null; latest: string | null; current: string | null; checkedAt: number | null }

export interface OvenRelease { tag: string; current: string | null; latest: string; upToDate: boolean; updatedAt: string; notes: string }

export interface About {
  pi: string | null;
  oven: string;
  packages: { source: string; name: string; version?: string; description?: string }[];
  mcp: { name: string; transport?: string; tools?: number; prompts?: number; configured: boolean; disabled: boolean }[];
}

/** What pi-updater offered at the last session start: pi `[current, latest]` and/or package names. */
export interface UpdateOffer { pi?: [string, string]; extensions: string[] }
export interface UpdateStatus { offer: UpdateOffer | null; running: boolean }
export type UpdateTarget = 'pi' | 'extensions' | 'all';
export interface UpdateResult { ok: boolean; code: number | null; output: string }

export interface SessionTree { path: string; cwd: string; leafId: string | null; entries: Entry[]; labels: Record<string, string> }
export interface Branched { path: string; id: string; cwd: string; entries: number; text: string | null }
export interface GitFile { path: string; status: string; staged: boolean; unstaged: boolean; added: number | null; removed: number | null }
export interface GitCommit { hash: string; subject: string; when: number }
export interface GitOverview { repo: boolean; branch: string | null; root: string | null; files: GitFile[]; truncated: boolean; commits: GitCommit[] }

async function callText(path: string): Promise<string> {
  const headers = new Headers();
  if (auth.token) headers.set('Authorization', `Bearer ${auth.token}`);
  headers.set('X-Pi-Oven-Lang', currentLang()); // the server words its errors in the page's language
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
  /** A copy of the session that belongs to another folder (the original stays). */
  moveSession: (path: string, cwd: string) =>
    call<Branched>('/api/sessions/move', { method: 'POST', body: JSON.stringify({ path, cwd }) }),
  branchSession: (path: string, leaf: string | null, textOf?: string) =>
    call<Branched>('/api/sessions/branch', { method: 'POST', body: JSON.stringify({ path, leaf, textOf }) }),
  git: (id: string) => call<GitOverview>(`/api/runtimes/${id}/git`),
  gitStage: (id: string, paths: string[], stage: boolean) =>
    call<void>(`/api/runtimes/${id}/git/stage`, { method: 'POST', body: JSON.stringify({ paths, stage }) }),
  gitCommit: (id: string, message: string) =>
    call<{ hash: string }>(`/api/runtimes/${id}/git/commit`, { method: 'POST', body: JSON.stringify({ message }) }),
  /** The newest saved state of an extension (`appendEntry`) on the branch the session stands on, or null. */
  sessionState: (path: string, type: string) => call<{ data: unknown }>(`/api/sessions/state?path=${encodeURIComponent(path)}&type=${encodeURIComponent(type)}`),
  tailSession: (path: string, custom?: string) =>
    call<{ lines: string[] }>(`/api/sessions/tail?path=${encodeURIComponent(path)}&n=25${custom ? `&custom=${encodeURIComponent(custom)}` : ''}`),
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
  info: () => call<{ version: string; build?: string | null; home: string; sessionsDir: string }>('/api/info'),
  ovenCheck: () => call<OvenRelease>('/api/oven/update'),
  ovenStatus: () => call<OvenStatus>('/api/oven/status'),
  setOvenAuto: (enabled: boolean) => call<OvenStatus>('/api/oven/auto', { method: 'PUT', body: JSON.stringify({ enabled }) }),
  ovenApply: () => call<{ restarting: boolean; latest: string }>('/api/oven/update', { method: 'POST' }),
  sessions: () => call<SessionSummary[]>('/api/sessions'),
  /** Name a session that is not open (an empty name clears it). An open one is renamed through its own pi. */
  renameSession: (path: string, name: string) => call<void>('/api/sessions/rename', { method: 'POST', body: JSON.stringify({ path, name }) }),
  /** `close`: an open session is closed first (wherever it is open) instead of refused. */
  deleteSession: (path: string, close = false) =>
    call<void>(`/api/sessions?path=${encodeURIComponent(path)}${close ? '&close=true' : ''}`, { method: 'DELETE' }),
  runtimes: () => call<RuntimeInfo[]>('/api/runtimes'),
  openRuntime: (body: { cwd: string; sessionPath?: string }) =>
    call<RuntimeInfo>('/api/runtimes', { method: 'POST', body: JSON.stringify(body) }),
  restartRuntime: (id: string, sessionPath?: string) =>
    call<RuntimeInfo>(`/api/runtimes/${id}/restart`, { method: 'POST', body: JSON.stringify({ sessionPath }) }),
  tasks: (id: string) => call<BgTask[]>(`/api/runtimes/${id}/tasks`),
  about: () => call<About>('/api/about'),
  updates: () => call<UpdateStatus>('/api/update'),
  runUpdate: (target: UpdateTarget) =>
    call<UpdateResult>('/api/update', { method: 'POST', body: JSON.stringify({ target }) }),
  limits: () => call<{ items: LimitWire[] }>('/api/limits'),
  resumeSettings: () => call<ResumeSettings>('/api/resume'),
  setResumeSettings: (s: ResumeSettings) => call<ResumeSettings>('/api/resume', { method: 'PUT', body: JSON.stringify(s) }),
  resumeAct: (id: string, action: 'now' | 'stop') =>
    call<void>(`/api/runtimes/${id}/resume`, { method: 'POST', body: JSON.stringify({ action }) }),
  longMode: () => call<LongMode>('/api/retry'),
  setLongMode: (enabled: boolean) =>
    call<LongMode>('/api/retry', { method: 'PUT', body: JSON.stringify({ enabled }) }),
  longAct: (id: string, action: 'now' | 'stop') =>
    call<void>(`/api/runtimes/${id}/retry`, { method: 'POST', body: JSON.stringify({ action }) }),
  async taskOutput(id: string, task: string, tail = 24 * 1024): Promise<{ text: string; total: number }> {
    const headers = new Headers();
    if (auth.token) headers.set('Authorization', `Bearer ${auth.token}`);
    headers.set('X-Pi-Oven-Lang', currentLang());
    const res = await fetch(`${serverUrl.value}/api/runtimes/${id}/tasks/${task}/output?tail=${tail}`, { headers });
    if (res.status === 404) return { text: '', total: 0 };       // nothing written yet
    if (!res.ok) throw new Error(`${res.status} ${res.statusText}`);
    return { text: await res.text(), total: Number(res.headers.get('x-total-bytes') ?? 0) };
  },
  closeRuntime: (id: string) => call<void>(`/api/runtimes/${id}`, { method: 'DELETE' }),
  mkdir: (parent: string, name: string) =>
    call<DirListing>('/api/fs/mkdir', { method: 'POST', body: JSON.stringify({ parent, name }) }),
  browse: (path?: string, hidden = false) =>
    call<DirListing>(`/api/fs?${new URLSearchParams({ ...(path ? { path } : {}), hidden: String(hidden) })}`),
  /** An image file's bytes, for an `<img>` (which cannot send the token in a header). `v` busts the cache. */
  rawUrl(id: string, path: string, v = 0) {
    const base = (serverUrl.value || location.origin).replace(/\/+$/, '');
    const q = new URLSearchParams({ path, v: String(v), ...(auth.token ? { token: auth.token } : {}) });
    return `${base}/api/runtimes/${id}/files/raw?${q}`;
  },
  wsUrl(id: string) {
    const base = new URL(serverUrl.value || location.origin);
    const proto = base.protocol === 'https:' ? 'wss' : 'ws';
    const q = auth.token ? `?token=${encodeURIComponent(auth.token)}` : '';
    return `${proto}://${base.host}/api/runtimes/${id}/ws${q}`;
  },
};
