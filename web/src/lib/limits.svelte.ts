// Subscription usage limits: the latest numbers the server has, shared by every session on this page.
import { api } from './api';
import { fromWire, type Limit, type LimitProblem } from './limits';

class Limits {
  items = $state<Record<string, Limit>>({});
  /** The other saved accounts' plans (Settings → Accounts), each with its own numbers when asked. */
  others = $state<Limit[]>([]);
  /** Providers that could not be asked just now (shown instead of a chip that silently stays as it was). */
  problems = $state<Record<string, LimitProblem>>({});
  /** The account behind a provider's plan just changed (switched in Settings, or Baton Touch at a limit). The info
      tab's plan card plays its change on it. */
  swap = $state<{ n: number; provider: string; who?: string | null } | null>(null);
  #swappedAt = 0;
  swapped(provider: string, who?: string | null) {
    // the same change heard twice (the page that switched, then every session's notice of it): played once
    const now = Date.now();
    if (this.swap?.provider === provider && this.swap.who === who && now - this.#swappedAt < 5000) return;
    this.#swappedAt = now;
    this.swap = { n: (this.swap?.n ?? 0) + 1, provider, who };
    void this.refresh(true);
  }
  /** Ticks so countdowns and resets move on their own. */
  now = $state(Date.now());
  #started = false;
  #busy = false;

  /** Called by whatever shows the limits: starts the clock and the first fetch once. */
  ensure() {
    if (this.#started) return;
    this.#started = true;
    void this.refresh(true);
    setInterval(() => (this.now = Date.now()), 30_000);
    // while in view, ask the provider itself every 5 minutes (no message needed); coming back asks at once
    setInterval(() => {
      if (document.visibilityState === 'visible') void this.refresh(true);
    }, 5 * 60_000);
    document.addEventListener('visibilitychange', () => {
      if (document.visibilityState === 'visible') void this.refresh(true);
    });
  }

  #lastAsk = 0;

  /** `ask`: have the server ask the provider now; otherwise read what it has. Opening the limits again and again asks
      at most once a minute from here (and the server keeps all pages together to the same). */
  /** A refresh asked for while another was running: done right after it, not dropped (a model switch landed here). */
  #again: { ask: boolean; urgent: boolean } | null = null;

  async refresh(ask = false, urgent = false) {
    if (this.#busy) {
      this.#again = { ask: ask || !!this.#again?.ask, urgent: urgent || !!this.#again?.urgent };
      return;
    }
    this.#busy = true;
    try {
      const now0 = Date.now();
      // `urgent`: a model of another plan was just picked, so ask without waiting out the minute (the server still paces)
      const asking = ask && (urgent || now0 - this.#lastAsk >= 60_000);
      if (asking) this.#lastAsk = now0;
      const r = asking ? await api.refreshLimits().catch(() => api.limits()) : await api.limits();
      const now = Date.now();
      this.now = now;
      this.items = Object.fromEntries(r.items.map((w) => [w.provider, fromWire(w, now)]));
      this.others = (r.others ?? []).map((w) => fromWire(w, now));
      this.problems = Object.fromEntries((r.problems ?? []).map((p) => [p.provider, { provider: p.provider, why: p.why, retryAt: now + p.retryInMs }]));
    } catch {
      // an older server, or offline: nothing to show
    } finally {
      this.#busy = false;
      const next = this.#again;
      this.#again = null;
      if (next) void this.refresh(next.ask, next.urgent);
    }
  }
}

export const limits = new Limits();
