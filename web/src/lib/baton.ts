// What Baton Touch did, as one line for the chat: from the server's `oven_baton` frame and from the note it keeps in
// the session (`oven-baton` custom message, the same fields), so a reload shows the same line.
import { t } from './i18n';
import { clock } from './util.ts';

export interface BatonNote {
  provider?: string;
  email?: string | null;
  plan?: string | null;
  model?: string;
  /// every account was limited: the one taking over is free then (ms)
  at?: number;
  /// back on the model it handed over from
  back?: boolean;
  /// what it was before: the account's email, or the model
  was?: string | null;
}

export type BatonKind = 'swap' | 'later' | 'model' | 'back';
export const batonKind = (n: BatonNote): BatonKind => (n.back ? 'back' : n.at ? 'later' : n.model ? 'model' : 'swap');

export const BATON_NOTE = 'oven-baton';

export function batonText(n: BatonNote): string {
  const who = n.email || n.model || n.provider || '';
  if (n.back) return t('accounts.batonBack', { who });
  if (n.at) return t('accounts.batonLater', { who, at: clock(n.at) });
  if (n.model) return t('accounts.batonModel', { who });
  return t('accounts.batonNotice', { who });
}
