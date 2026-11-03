import { t } from './i18n';
// How fast an answer is coming: the wait before the first token (TTFT) and the rate once it flows (tokens a second).
// pi's events carry no timing, so it is measured here as they arrive. The rate uses the output token count pi
// sends with each piece when the provider reports it, and an estimate from the characters otherwise.

export interface Speed {
  /** Milliseconds from the request to the first token. */
  ttft?: number;
  /** Tokens a second while generating. */
  tps?: number;
  /** The rate is an estimate from characters, not a count of tokens. */
  est: boolean;
  /** When the first token arrived (performance.now()). */
  first?: number;
  tokens?: number;
}

/** Hangul and CJK take about 0.7 of a token a character; Latin text and code about a quarter. */
export function estimateTokens(text: string): number {
  let n = 0;
  for (const ch of text) n += /[ᄀ-ᇿ㄰-㆏가-힯぀-ヿ一-鿿]/.test(ch) ? 0.7 : 0.27;
  return n;
}

/** Shorter than this, a rate is noise. */
const MIN_SPAN_MS = 400;

export class StreamTimer {
  first?: number;
  last?: number;
  #est = 0;
  readonly ref: number;

  constructor(ref: number) {
    this.ref = ref;
  }

  delta(now: number, text: string) {
    this.first ??= now;
    this.last = now;
    this.#est += estimateTokens(text);
  }

  /** `counted`: tokens the provider has already reported for this message, when it reports them as it goes. */
  snapshot(now: number, counted?: number): Speed {
    if (this.first === undefined) return { est: true };
    const tokens = counted && counted > 0 ? counted : undefined;
    const span = (this.last ?? now) - this.first;
    const n = tokens ?? this.#est;
    return { ttft: Math.max(0, this.first - this.ref), tps: span >= MIN_SPAN_MS && n >= 2 ? n / (span / 1000) : undefined, est: tokens === undefined, first: this.first, tokens };
  }

  /** The message is over: with the real token count the average is exact. */
  finish(now: number, outputTokens?: number): Speed {
    if (this.first === undefined) return { est: true };
    const span = (this.last ?? now) - this.first;
    const exact = outputTokens !== undefined && outputTokens > 0;
    const n = exact ? outputTokens! : this.#est;
    return { ttft: Math.max(0, this.first - this.ref), tps: span >= MIN_SPAN_MS && n >= 2 ? n / (span / 1000) : undefined, est: !exact, first: this.first, tokens: exact ? outputTokens : undefined };
  }
}


export const formatMs = (ms: number) => (ms < 1000 ? `${Math.round(ms)}ms` : t('speed.secs', { n: (ms / 1000).toFixed(ms < 10_000 ? 1 : 0) }));
export const formatRate = (r: number) => (r < 10 ? r.toFixed(1) : String(Math.round(r)));
