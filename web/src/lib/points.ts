// "Points": the things worth keeping in view while building: addresses a server printed, tokens it
// handed out. Found by pattern in what the conversation already contains; nothing leaves the browser.
// A heuristic, so it can miss one or flag one that is not; a person can always pin by hand.

export interface UrlPoint {
  kind: 'url';
  /** Normalised: always has a scheme. */
  value: string;
  host: string;
  port: string;
  /** Times it appeared; and the item index it last appeared in (bigger is newer). */
  count: number;
  last: number;
}
export interface SecretPoint {
  kind: 'secret';
  /** What it was found next to (`token`, `Bearer`, `ghp_`…). */
  label: string;
  value: string;
  count: number;
  last: number;
}
export type Point = UrlPoint | SecretPoint;

const count = (s: string, ch: string) => s.split(ch).length - 1;

/** Trailing punctuation that belongs to the sentence, not the address; a ")" stays if one opened inside. */
export function trimUrl(raw: string): string {
  let u = raw;
  while (u) {
    const last = u[u.length - 1];
    const open = ({ ')': '(', ']': '[', '}': '{' } as Record<string, string>)[last];
    if (/[.,;:!?'"”’»>]/.test(last) || (open && count(u, last) > count(u, open))) u = u.slice(0, -1);
    else break;
  }
  return u;
}

const SCHEME = /\bhttps?:\/\/[^\s<>"'`\\^|{}]+/gi;
const OCTET = '(?:25[0-5]|2[0-4]\\d|1?\\d?\\d)';
const BARE = new RegExp(`(?<![\\w./@:-])(?:localhost|${OCTET}(?:\\.${OCTET}){3}):\\d{2,5}(?![\\w])(?:/[^\\s<>"'\`\\\\^|{}]*)?`, 'gi');

function parseUrl(u: string): { value: string; host: string; port: string } | null {
  try {
    const url = new URL(u);
    if (!url.hostname) return null;
    return { value: u, host: url.hostname, port: url.port };
  } catch {
    return null;
  }
}

export function findUrls(text: string): { value: string; host: string; port: string }[] {
  const out: { value: string; host: string; port: string }[] = [];
  const taken: [number, number][] = [];
  for (const m of text.matchAll(SCHEME)) {
    const v = trimUrl(m[0]);
    const p = parseUrl(v);
    if (p) out.push(p);
    taken.push([m.index!, m.index! + m[0].length]);
  }
  for (const m of text.matchAll(BARE)) {
    if (taken.some(([a, b]) => m.index! >= a && m.index! < b)) continue;
    const v = trimUrl('http://' + m[0]);
    const p = parseUrl(v);
    if (p) out.push(p);
  }
  return out;
}

// Every repetition is bounded: an unbounded `[…]*` before a keyword makes the engine retry from each
// position, which on a long dotted or repeated string takes minutes. Real names are short.
const NAME = String.raw`[A-Za-z0-9_.-]{0,32}(?:token|secret|api[_-]?key|apikey|passwd|password)[A-Za-z0-9_.-]{0,32}`;
const VALUE = String.raw`[A-Za-z0-9._~+/=-]{12,512}`;
const SECRET_PATTERNS: { label: (m: RegExpMatchArray) => string; re: RegExp; group: number }[] = [
  // `token=abc…`, `"api_key": "abc…"`, `export API_TOKEN=abc…`, `#token=abc…`
  { re: new RegExp(String.raw`(?<![A-Za-z0-9])(${NAME})["']?\s*[=:：]\s*["']?(${VALUE})`, 'gi'), group: 2, label: (m) => m[1].replace(/^[._-]+/, '') },
  // `토큰: abc…`, `비밀번호는 abc…` (a particle or a colon after the word; the value still has to look like one)
  { re: new RegExp(String.raw`(토큰|비밀번호|비번|암호|패스워드|시크릿|인증키)\s{0,2}(?:[:：=]|(?:은|는|이|가)?\s{0,2}[:：=]?)\s{0,2}["'\x60]?(${VALUE})`, 'g'), group: 2, label: (m) => m[1] },
  // a UUID on its own (not inside a path, a file name or a longer word)
  { re: /(?<![\w./\\-])([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})(?![\w/\\-]|\.\w)/gi, group: 1, label: () => 'UUID' },
  // `--token abc…`
  { re: new RegExp(String.raw`--(${NAME})[ =](${VALUE})`, 'gi'), group: 2, label: (m) => m[1] },
  { re: /\bBearer\s+([A-Za-z0-9._~+/=-]{12,512})/g, group: 1, label: () => 'Bearer' },
  { re: /\b(gh[pousr]_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{40,})\b/g, group: 1, label: () => 'GitHub' },
  { re: /\b(sk-[A-Za-z0-9_-]{20,})\b/g, group: 1, label: () => 'sk-' },
  { re: /\b(AKIA[0-9A-Z]{16})\b/g, group: 1, label: () => 'AWS' },
  { re: /\b(xox[abprs]-[A-Za-z0-9-]{10,})\b/g, group: 1, label: () => 'Slack' },
  { re: /\b(eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,})\b/g, group: 1, label: () => 'JWT' },
];

/** Words that follow "token" in prose and are not a value. */
const NOT_VALUES = /^(?:undefined|required|string|optional|placeholder|your[-_]?token|example|xxxxxxxx+|\*{6,}|\.{3,}|true|false|null)$/i;

/** A secret mixes character kinds; an English-looking word or a plain path does not. */
function looksSecret(v: string): boolean {
  if (/^(gh[pousr]_|github_pat_|sk-|AKIA|xox|eyJ)/.test(v)) return true;
  if (/^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/i.test(v)) return true;
  if (v.includes('/') && !/\d{3,}/.test(v)) return false;
  return /\d/.test(v) || (/[A-Z]/.test(v) && /[a-z]/.test(v));
}

/** A rule the person wrote (Settings → Pin 감지 규칙): what matches (group 1 if there is one, else all of it) becomes a pin. */
export interface CompiledRule { name: string; re: RegExp }

const NESTED = /\((?:[^()\\]|\\.)*[+*](?:[^()\\]|\\.)*\)\s*(?:[+*]|\{\d*,\d*\})/;

/** Why a pattern is refused, or null when it is fine. Backtracking can't be interrupted, so the shapes that blow up are turned away. */
export function ruleProblem(pattern: string): string | null {
  if (!pattern.trim()) return '패턴을 입력하세요.';
  if (pattern.length > 200) return '패턴이 너무 깁니다(200자까지).';
  if (NESTED.test(pattern)) return '반복 안에 반복이 있는 패턴은 쓸 수 없습니다. 예: (a+)+';
  try {
    const re = new RegExp(pattern, 'g');
    if (re.test('')) return '빈 글자에도 맞는 패턴입니다.';
  } catch (e) {
    return '정규식이 올바르지 않습니다.';
  }
  return null;
}

export function compileRule(name: string, pattern: string): CompiledRule | null {
  if (ruleProblem(pattern)) return null;
  return { name: name.trim() || '규칙', re: new RegExp(pattern, 'g') };
}

export function findSecrets(text: string, rules: CompiledRule[] = []): { label: string; value: string }[] {
  const out = new Map<string, { label: string; value: string }>();
  for (const { name, re } of rules) {
    let n = 0;
    for (const m of text.matchAll(re)) {
      const value = (m[1] ?? m[0]).trim();
      if (value && value.length <= 512 && !out.has(value)) out.set(value, { label: name, value });
      if (++n >= 200) break;
    }
  }
  for (const { re, group, label } of SECRET_PATTERNS) {
    for (const m of text.matchAll(re)) {
      const value = m[group].replace(/[.,;:)\]}]+$/, '');
      if (value.length < 12 || NOT_VALUES.test(value)) continue;
      if (!looksSecret(value)) continue;
      if (!out.has(value)) out.set(value, { label: label(m), value });
    }
  }
  return [...out.values()];
}

/** abcd••••wxyz: enough to tell two tokens apart without showing one. */
export function mask(value: string): string {
  return value.length <= 10 ? '•'.repeat(value.length) : `${value.slice(0, 4)}••••${value.slice(-4)}`;
}

/** The address as shown: any token inside it is masked, the rest stays readable. */
export function displayUrl(url: string): string {
  return url.replace(/([?&#]\w*(?:token|key|secret|auth|password)\w*=)([^&#\s]{8,})/gi, (_, k, v) => k + mask(v));
}

const LOCAL = new Set(['localhost', '127.0.0.1', '0.0.0.0', '::1', '[::1]']);
export const isLocalHost = (h: string) => LOCAL.has(h);

/** From another device `localhost` is the wrong machine: swap in the host this page itself was opened on. */
export function reachable(url: string, pageHost: string): string {
  if (!pageHost || LOCAL.has(pageHost)) return url;
  try {
    const u = new URL(url);
    if (!LOCAL.has(u.hostname)) return url;
    u.hostname = pageHost;
    return u.toString();
  } catch {
    return url;
  }
}

/** Text of an item that can hold an address or a token. Capped: a build log can be megabytes. */
const CAP = 60_000;
const cut = (s: string) => (s.length > CAP ? s.slice(0, CAP / 2) + '\n' + s.slice(-CAP / 2) : s);

export function itemTexts(item: any): string[] {
  switch (item.kind) {
    case 'user': return [item.text];
    case 'bash': return [item.command, item.output];
    case 'summary': return [];
    case 'assistant': {
      const out: string[] = [];
      for (const b of item.blocks ?? []) {
        if (b.type === 'text') out.push(b.text);
        else if (b.type === 'tool') out.push(JSON.stringify(b.args ?? {}), b.output ?? '');
      }
      return out;
    }
    default: return [];
  }
}

const signature = (item: any) => {
  let n = item.kind === 'assistant' ? item.live : 0;
  for (const t of itemTexts(item)) n = `${n}:${t.length}`;
  return String(n);
};

/** Scans only what changed since the last call: re-reading a long session on every streamed token would be wasteful. */
export class PointScanner {
  #cache = new Map<number, { sig: string; urls: ReturnType<typeof findUrls>; secrets: ReturnType<typeof findSecrets> }>();
  #rules: CompiledRule[] = [];

  /** New rules mean everything has to be read again. */
  setRules(rules: CompiledRule[]) {
    this.#rules = rules;
    this.#cache.clear();
  }

  scan(items: any[]): { urls: UrlPoint[]; secrets: SecretPoint[] } {
    const urls = new Map<string, UrlPoint>();
    const secrets = new Map<string, SecretPoint>();
    const seen = new Set<number>();
    items.forEach((item, index) => {
      seen.add(item.key);
      const sig = signature(item);
      let c = this.#cache.get(item.key);
      if (!c || c.sig !== sig) {
        const text = itemTexts(item).map(cut).join('\n');
        c = { sig, urls: findUrls(text), secrets: findSecrets(text, this.#rules) };
        this.#cache.set(item.key, c);
      }
      for (const u of c.urls) {
        const prev = urls.get(u.value);
        if (prev) { prev.count++; prev.last = index; } else urls.set(u.value, { kind: 'url', ...u, count: 1, last: index });
      }
      for (const s of c.secrets) {
        const prev = secrets.get(s.value);
        if (prev) { prev.count++; prev.last = index; } else secrets.set(s.value, { kind: 'secret', ...s, count: 1, last: index });
      }
    });
    for (const k of this.#cache.keys()) if (!seen.has(k)) this.#cache.delete(k);
    const newest = (a: Point, b: Point) => b.last - a.last;
    return { urls: [...urls.values()].sort(newest), secrets: [...secrets.values()].sort(newest) };
  }
}
