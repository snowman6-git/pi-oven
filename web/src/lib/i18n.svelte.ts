// The chosen language, kept reactive. Every language is one JSON file in `src/locales/` (see the README there); a new
// file is picked up at the next build with no code change.
import { register, type Tree } from './i18n';

export { t, locale, type Key, type Params } from './i18n';

const files = import.meta.glob<Tree>('../locales/*.json', { eager: true, import: 'default' });
const dicts: Record<string, Tree> = {};
for (const [path, d] of Object.entries(files)) dicts[path.replace(/^.*\/|\.json$/g, '')] = d;

/** Languages there is a file for, by the name the file gives itself. */
export const LANGS: { code: string; name: string }[] = Object.entries(dicts)
  .map(([code, d]) => ({ code, name: String((d._meta as Tree | undefined)?.name ?? code) }))
  .sort((a, b) => a.name.localeCompare(b.name));

const STORE = 'pi-oven.lang';

function pick(): string {
  try {
    const saved = localStorage.getItem(STORE);
    if (saved && dicts[saved]) return saved;
  } catch { /* private mode */ }
  for (const l of navigator.languages ?? [navigator.language]) {
    const low = l.toLowerCase();
    if (dicts[low]) return low;
    const base = low.split('-')[0];
    if (dicts[base]) return base;
  }
  return dicts.en ? 'en' : 'ko';
}

export const i18n = $state({ lang: pick() });
register(dicts, () => i18n.lang);
document.documentElement.lang = i18n.lang;

export function setLang(code: string) {
  if (!dicts[code]) return;
  i18n.lang = code;
  try { localStorage.setItem(STORE, code); } catch { /* private mode */ }
  document.documentElement.lang = code;
}
