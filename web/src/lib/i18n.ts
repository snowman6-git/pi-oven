// Interface language: lookup and formatting. Plain TypeScript so the pure modules (and their tests) can use it; the
// app side (`i18n.svelte.ts`) finds the language files, keeps the chosen language reactive and registers both here.
// Without that (tests) the text is Korean, the source language.
import ko from '../locales/ko.json';

/** A value with forms by count, chosen with `Intl.PluralRules`: `{ "one": "1 file", "other": "{n} files" }`. */
type Plural = { other: string } & Partial<Record<'zero' | 'one' | 'two' | 'few' | 'many', string>>;
export type Tree = { [k: string]: string | Plural | Tree };

// every key of ko.json as "a.b.c" (a plural object is one key)
type Keys<T, P extends string = ''> = {
  [K in keyof T & string]: K extends '_meta' ? never
    : T[K] extends string ? `${P}${K}`
    : T[K] extends { other: string } ? `${P}${K}`
    : Keys<T[K], `${P}${K}.`>;
}[keyof T & string];
export type Key = Keys<typeof ko>;
export type Params = Record<string, string | number>;

let dicts: Record<string, Tree> = { ko: ko as Tree };
let current: () => string = () => 'ko';

export function register(all: Record<string, Tree>, lang: () => string) {
  dicts = all;
  current = lang;
}

function find(d: Tree | undefined, key: string): string | Plural | undefined {
  let cur: unknown = d;
  for (const part of key.split('.')) {
    if (!cur || typeof cur !== 'object') return undefined;
    cur = (cur as Tree)[part];
  }
  return typeof cur === 'string' || (cur && typeof cur === 'object' && 'other' in cur) ? (cur as string | Plural) : undefined;
}

const plurals = new Map<string, Intl.PluralRules>();

/** The text for `key` in the current language (falling back to English, then Korean), with `{name}` filled in from
    `params`. A plural value picks its form by `params.n`. Read inside a component, it follows language changes. */
export function t(key: Key, params?: Params): string {
  const lang = current();
  const v = find(dicts[lang], key) ?? find(dicts.en, key) ?? find(ko as Tree, key);
  if (v === undefined) return key;
  let s: string;
  if (typeof v === 'string') s = v;
  else {
    const n = Number(params?.n ?? 0);
    let rules = plurals.get(lang);
    if (!rules) plurals.set(lang, (rules = new Intl.PluralRules(lang)));
    s = (n === 0 && v.zero) || v[rules.select(n)] || v.other;
  }
  return params ? s.replace(/\{(\w+)\}/g, (m, k: string) => (k in params ? String(params[k]) : m)) : s;
}

/** The current language code (`ko`, `en`…). */
export const currentLang = () => current();

/** The BCP 47 tag for dates and numbers. */
export function locale(): string {
  const lang = current();
  const meta = dicts[lang]?._meta as Tree | undefined;
  return typeof meta?.locale === 'string' ? meta.locale : lang;
}
