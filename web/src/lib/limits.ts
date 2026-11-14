// Subscription usage limits: the pure part (what the server sends → what the ring and the cards show).
import { locale, t } from './i18n';

export type LimitWindowWire = { minutes: number; used: number; resetInMs: number | null };
export type LimitWire = { provider: string; plan: string | null; ageMs: number; limitReached: boolean; resets?: number | null; windows: LimitWindowWire[] };

export type LimitWindow = { minutes: number; used: number; resetAt: number | null };
export type Limit = { provider: string; plan: string | null; at: number; limitReached: boolean; resets: number | null; windows: LimitWindow[] };

/** A window as it stands now: past its reset it starts again from nothing. Providers send what is used; the page
 * shows what is left (`left`), which is what one wants to know before sending more. */
export type WindowNow = { minutes: number; used: number; left: number; resetAt: number | null; reached: boolean; level: 'ok' | 'warm' | 'full' };

export function fromWire(w: LimitWire, now: number): Limit {
  return {
    provider: w.provider,
    plan: w.plan,
    at: now - w.ageMs,
    limitReached: w.limitReached,
    resets: w.resets ?? null,
    windows: w.windows.map((x) => ({ minutes: x.minutes, used: x.used, resetAt: x.resetInMs == null ? null : now + x.resetInMs })),
  };
}

function windowNow(w: LimitWindow, now: number): WindowNow {
  const over = w.resetAt != null && now >= w.resetAt;
  const used = over ? 0 : Math.max(0, Math.min(100, w.used));
  const reached = used >= 99.5;
  const left = 100 - used;
  return { minutes: w.minutes, used, left, resetAt: over ? null : w.resetAt, reached, level: reached ? 'full' : left < 25 ? 'warm' : 'ok' };
}

/** The short window (5 hours) and the long one (a week): the ring's right and left halves. */
export function halves(l: Limit, now: number): { short?: WindowNow; long?: WindowNow } {
  const ws = [...l.windows].sort((a, b) => a.minutes - b.minutes).map((w) => windowNow(w, now));
  if (ws.length === 1) return ws[0].minutes > 1440 ? { long: ws[0] } : { short: ws[0] };
  return { short: ws[0], long: ws[ws.length - 1] };
}

export function providerName(p: string): string {
  if (p === 'openai-codex' || p === 'openai-chatgpt') return 'ChatGPT';
  if (p === 'anthropic') return 'Claude';
  return p;
}

export function planName(p: string | null): string {
  return p ? p.charAt(0).toUpperCase() + p.slice(1) : '';
}

/** "5시간", "주간", or the length itself for windows we do not know by name. */
export function windowName(minutes: number): string {
  if (minutes === 300) return t('limits.w5h');
  if (minutes === 10080) return t('limits.week');
  if (minutes % 1440 === 0) return t('limits.days', { n: minutes / 1440 });
  return t('limits.hours', { n: Math.round(minutes / 60) });
}

/** Time to a reset, in words: "4시간 38분", "12분", "3일". */
export function until(resetAt: number, now: number): string {
  const m = Math.max(1, Math.ceil((resetAt - now) / 60000));
  if (m < 60) return t('limits.m', { m });
  const h = Math.floor(m / 60);
  if (h < 48) return t('limits.hm', { h, m: m % 60 });
  return t('limits.days', { n: Math.round(h / 24) });
}

/** The same, short enough for the chip: "1:12", "2일". */
export function untilShort(resetAt: number, now: number): string {
  const m = Math.max(1, Math.ceil((resetAt - now) / 60000));
  if (m < 48 * 60) return `${Math.floor(m / 60)}:${String(m % 60).padStart(2, '0')}`;
  return t('limits.days', { n: Math.round(m / 1440) });
}

export function resetDate(resetAt: number, time = true): string {
  const o: Intl.DateTimeFormatOptions = { month: 'short', day: 'numeric', weekday: 'short' };
  return new Date(resetAt).toLocaleString(locale(), time ? { ...o, hour: '2-digit', minute: '2-digit', hour12: false } : o);
}
