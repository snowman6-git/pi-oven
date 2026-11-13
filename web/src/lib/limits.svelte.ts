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
    void this.refresh();
    setInterval(() => (this.now = Date.now()), 30_000);
    // other devices' sessions bring new numbers too
    setInterval(() => {
      if (document.visibilityState === 'visible') void this.refresh();
    }, 5 * 60_000);
    document.addEventListener('visibilitychange', () => {
      if (document.visibilityState === 'visible') void this.refresh();
    });
  }

  async refresh() {
    if (this.#busy) return;
    this.#busy = true;
    try {
      const r = await api.limits();
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
