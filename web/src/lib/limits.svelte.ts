// Subscription usage limits: the latest numbers the server has, shared by every session on this page.
import { api } from './api';
import { fromWire, type Limit } from './limits';

class Limits {
  items = $state<Record<string, Limit>>({});
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
  async refresh(ask = false) {
    if (this.#busy) return;
    this.#busy = true;
    try {
      const now0 = Date.now();
      const asking = ask && now0 - this.#lastAsk >= 60_000;
      if (asking) this.#lastAsk = now0;
      const r = asking ? await api.refreshLimits().catch(() => api.limits()) : await api.limits();
      const now = Date.now();
      this.now = now;
      this.items = Object.fromEntries(r.items.map((w) => [w.provider, fromWire(w, now)]));
    } catch {
      // an older server, or offline: nothing to show
    } finally {
      this.#busy = false;
    }
  }
}

export const limits = new Limits();
