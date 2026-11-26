// What the server did while nobody was working, as one quiet line for the chat: the session's own note
// (`oven-rest` custom message from `/oven-note`, so a reload shows it too).
import { currentLang, t } from './i18n';
import { compactNumber } from './util.ts';

export interface RestNote {
  reason?: 'idle' | 'limit';
  action?: 'compact' | 'summary' | 'commit' | 'custom';
  before?: number;
  after?: number | null;
  /// the plan's share left when it acted (%)
  left?: number | null;
}

export const REST_NOTE = 'oven-rest';

/** 410000 → "41만" (Korean counts by 만), "410k" elsewhere. */
export function tokens(n: number): string {
  if (currentLang() !== 'ko') return compactNumber(n);
  if (n >= 100_000) return `${Math.round(n / 10_000)}만`;
  if (n >= 10_000) return `${(Math.round(n / 1_000) / 10)}만`;
  return n.toLocaleString();
}

export function restText(n: RestNote): string {
  const size = n.before != null && n.after != null ? ` · ${tokens(n.before)} → ${tokens(n.after)}` : '';
  if (n.reason === 'limit') return t('compact.line.limit', { left: n.left ?? '', action: t(({ compact: 'compact.line.action.compact', summary: 'compact.line.action.summary', commit: 'compact.line.action.commit', custom: 'compact.line.action.custom' } as const)[n.action ?? 'compact']) }) + size;
  return t('compact.line.idle') + size;
}
