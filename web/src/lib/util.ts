// eslint-disable-next-line no-control-regex
const ANSI = /\u001b\[[0-9;?]*[ -/]*[@-~]|\u001b\][^\u0007]*\u0007/g;

export const stripAnsi = (s: string) => s.replace(ANSI, '');

export function clock(ts?: number): string {
  if (!ts) return '';
  return new Date(ts).toLocaleTimeString('ko-KR', { hour: '2-digit', minute: '2-digit', hour12: false });
}

export function relativeTime(ms: number): string {
  const d = Date.now() - ms;
  const m = Math.round(d / 60000);
  if (m < 1) return '방금';
  if (m < 60) return `${m}분 전`;
  const h = Math.round(m / 60);
  if (h < 24) return `${h}시간 전`;
  const days = Math.round(h / 24);
  if (days < 7) return `${days}일 전`;
  return new Date(ms).toLocaleDateString('ko-KR', { month: 'numeric', day: 'numeric' });
}

export function duration(ms?: number): string {
  if (ms == null) return '';
  if (ms < 1000) return `${Math.round(ms)}ms`;
  if (ms < 60000) return `${(ms / 1000).toFixed(ms < 10000 ? 1 : 0)}s`;
  return `${Math.floor(ms / 60000)}m ${Math.round((ms % 60000) / 1000)}s`;
}

/** A running clock in whole seconds: 7s, 1m 05s, 1h 02m. */
export function runClock(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m ${String(s % 60).padStart(2, '0')}s`;
  return `${Math.floor(s / 3600)}h ${String(Math.floor((s % 3600) / 60)).padStart(2, '0')}m`;
}

export function compactNumber(n: number): string {
  if (n >= 1e6) return `${(n / 1e6).toFixed(1)}M`;
  if (n >= 1e3) return `${(n / 1e3).toFixed(n >= 1e4 ? 0 : 1)}k`;
  return String(n);
}

export function baseName(p: string): string {
  const parts = p.split('/').filter(Boolean);
  return parts[parts.length - 1] ?? '/';
}

export function tildePath(p: string, home: string): string {
  return home && p.startsWith(home) ? '~' + p.slice(home.length) : p;
}

/** Plain text of a content array (tool results, user messages). */
export function textOf(content: unknown): string {
  if (typeof content === 'string') return content;
  if (!Array.isArray(content)) return '';
  return content
    .filter((p: any) => p?.type === 'text')
    .map((p: any) => p.text)
    .join('\n');
}
