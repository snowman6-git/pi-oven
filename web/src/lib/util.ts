import { locale, t } from './i18n';
// eslint-disable-next-line no-control-regex
const ANSI = /\u001b\[[0-9;?]*[ -/]*[@-~]|\u001b\][^\u0007]*\u0007/g;

export const stripAnsi = (s: string) => s.replace(ANSI, '');

export function clock(ts?: number): string {
  if (!ts) return '';
  return new Date(ts).toLocaleTimeString(locale(), { hour: '2-digit', minute: '2-digit', hour12: false });
}

export function relativeTime(ms: number): string {
  const d = Date.now() - ms;
  const m = Math.round(d / 60000);
  if (m < 1) return t('time.now');
  if (m < 60) return t('time.min', { n: m });
  const h = Math.round(m / 60);
  if (h < 24) return t('time.hour', { n: h });
  const days = Math.round(h / 24);
  if (days < 7) return t('time.day', { n: days });
  return new Date(ms).toLocaleDateString(locale(), { month: 'numeric', day: 'numeric' });
}

export function duration(ms?: number): string {
  if (ms == null) return '';
  if (ms < 1000) return `${Math.round(ms)}ms`;
  if (ms < 60000) return `${(ms / 1000).toFixed(ms < 10000 ? 1 : 0)}s`;
  return `${Math.floor(ms / 60000)}m ${Math.round((ms % 60000) / 1000)}s`;
}

/** A running clock in whole seconds, one shape all the way so it never jumps: 0:07, 1:05, 12:05, 1:02:03. */
export function runClock(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  const ss = String(s % 60).padStart(2, '0');
  if (s < 3600) return `${Math.floor(s / 60)}:${ss}`;
  return `${Math.floor(s / 3600)}:${String(Math.floor((s % 3600) / 60)).padStart(2, '0')}:${ss}`;
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

/** The secret in `#token=…`, decoded. A password may hold any visible character, so nothing is cut off at a symbol. */
export function tokenFromHash(hash: string): string | null {
  const m = /(?:^#|&)token=([^&]*)/.exec(hash);
  if (!m || !m[1]) return null;
  try { return decodeURIComponent(m[1]).trim() || null; } catch { return m[1].trim() || null; }
}
