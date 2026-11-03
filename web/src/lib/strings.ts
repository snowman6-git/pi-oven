// Shared copy, read as `S.name`. The text itself is in `src/locales/<lang>.json` under "common"; this keeps the short
// form and follows the chosen language.
import type ko from '../locales/ko.json';
import { t, type Key } from './i18n';

export const S = new Proxy({} as { readonly [K in keyof typeof ko.common]: string }, {
  get: (_, k) => t(`common.${String(k)}` as Key),
});
