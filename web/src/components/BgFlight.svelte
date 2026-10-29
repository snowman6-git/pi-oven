<script lang="ts">
  import { tick } from 'svelte';
  import type { Chat } from '../lib/chat.svelte';

  let { chat }: { chat: Chat } = $props();
  let layer: HTMLElement;

  type P = { x: number; y: number };
  const TOTAL = 880;          // ms from pop to landing
  const POP = 0.24;           // share of the time spent popping out of the tool row

  const ease = {
    outBack: (t: number) => { const c = 1.9; return 1 + (c + 1) * (t - 1) ** 3 + c * (t - 1) ** 2; },
    inOut: (t: number) => (t < 0.5 ? 4 * t * t * t : 1 - (-2 * t + 2) ** 3 / 2),
  };
  const bez = (a: P, c: P, b: P, t: number): P => ({
    x: (1 - t) ** 2 * a.x + 2 * (1 - t) * t * c.x + t * t * b.x,
    y: (1 - t) ** 2 * a.y + 2 * (1 - t) * t * c.y + t * t * b.y,
  });
  const mid = (r: DOMRect): P => ({ x: r.left + r.width / 2, y: r.top + r.height / 2 });

  /** Where the task started: the dot in its tool row, or the bottom of the conversation if that row is out of view. */
  function source(toolId: string): P | null {
    const scroller = document.querySelector('.scroller')?.getBoundingClientRect();
    const dot = document.querySelector(`[data-tool="${CSS.escape(toolId)}"] .dot`)?.getBoundingClientRect();
    if (dot && scroller && dot.bottom > scroller.top && dot.top < scroller.bottom) return mid(dot);
    return scroller ? { x: scroller.left + scroller.width / 2, y: scroller.bottom - 24 } : null;
  }

  const calm = () => matchMedia('(prefers-reduced-motion: reduce)').matches;

  /** Where it lands: that task's own chip; with more than fit, the circle that stacks the rest. */
  function chipOf(taskId: string | null): HTMLElement | null {
    return taskId ? document.querySelector(`[data-bg-chip="${CSS.escape(taskId)}"]`) : null;
  }
  const stackOf = () => document.querySelector<HTMLElement>('[data-bg-stack]');
  /** The element this task lands on, if it is drawn yet. */
  const home = (taskId: string | null) => chipOf(taskId) ?? (taskId && !chipOf(taskId) ? stackOf() : null);
  function target(taskId: string | null): P | null {
    const el = home(taskId);
    const dot = (el?.matches('[data-bg-stack]') ? el : el?.querySelector('.dot'))?.getBoundingClientRect();
    if (dot && dot.width) return mid(dot);
    const c = document.querySelector('.composer')?.getBoundingClientRect();
    return c ? { x: c.left + 19, y: c.top - 8 - 15 } : null;
  }

  /** The chip appears with a pop of its own (or the circle gives a nudge as another task is stacked on it). */
  function land(taskId: string | null) {
    if (taskId) chat.bgLanding = chat.bgLanding.filter((x) => x !== taskId);
    const el = home(taskId);
    if (!el || calm()) return;
    el.animate(
      el.matches('[data-bg-stack]')
        ? [{ transform: 'scale(1)' }, { transform: 'scale(1.3)', offset: 0.4 }, { transform: 'scale(0.94)', offset: 0.7 }, { transform: 'scale(1)' }]
        : [{ transform: 'scale(0.55)', opacity: 0 }, { transform: 'scale(1.14)', opacity: 1, offset: 0.55 }, { transform: 'scale(1)', opacity: 1 }],
      { duration: 520, easing: 'cubic-bezier(0.3, 1.5, 0.5, 1)' },
    );
  }

  /** A task ended: its chip boings, with a ring that spreads in the colour of the outcome. */
  function celebrate(id: string, status: string) {
    const chip = chipOf(id);
    if (!chip || calm() || document.hidden) return;
    const css = getComputedStyle(document.documentElement);
    const tone = (status === 'completed' ? css.getPropertyValue('--ok') : css.getPropertyValue('--bad')).trim() || 'currentColor';
    chip.animate(
      [
        { transform: 'scale(1)' },
        { transform: 'scale(1.22)', offset: 0.28 },
        { transform: 'scale(0.94)', offset: 0.52 },
        { transform: 'scale(1.06)', offset: 0.74 },
        { transform: 'scale(1)' },
      ],
      { duration: 700, easing: 'cubic-bezier(0.3, 1.4, 0.5, 1)' },
    );
    chip.animate(
      [{ boxShadow: `0 0 0 0 color-mix(in srgb, ${tone} 60%, transparent)` }, { boxShadow: `0 0 0 12px color-mix(in srgb, ${tone} 0%, transparent)` }],
      { duration: 700, easing: 'ease-out' },
    );
  }

  /** An ember from one point to another: pops up out of its start, then arcs down to where `to()` says (asked each frame, so a target that moves is followed). Resolves when it arrives. */
  function ember(from: P, to: () => P | null, total: number, rise: number): Promise<void> {
    return new Promise((done) => {
      const first = to();
      if (!first) return done();
      const el = document.createElement('div');
      el.className = 'ember';
      layer.append(el);
      el.style.transform = `translate(${from.x - 4}px, ${from.y - 4}px)`;
      const popAt: P = { x: from.x + (first.x - from.x) * 0.05 - 10, y: from.y - rise };
      const t0 = performance.now();
      let lastTrail: P | null = null;     // where the last spark was left: sparks follow distance, not the clock, so the trail is even at any frame rate

      const trail = (p: P, size: number) => {
        const d = document.createElement('div');
        d.className = 'spark';
        d.style.cssText = `left:${p.x - size / 2}px;top:${p.y - size / 2}px;width:${size}px;height:${size}px`;
        layer.append(d);
        setTimeout(() => d.remove(), 420);
      };

      const frame = (now: number) => {
        const end = to();
        const k = Math.max(0, Math.min(1, (now - t0) / total));   // rAF's timestamp can be a hair older than t0
        if (!end) { el.remove(); return done(); }

        let pos: P, scale: number;
        if (k < POP) {                                   // out of its start: up and growing
          const u = ease.outBack(k / POP);
          pos = { x: from.x + (popAt.x - from.x) * u, y: from.y + (popAt.y - from.y) * u };
          scale = 1 + 1.3 * u;
        } else {                                         // an arc down to the target, shrinking back to dot size
          const u = ease.inOut((k - POP) / (1 - POP));
          const c: P = { x: Math.max(popAt.x, end.x) + 70, y: (popAt.y + end.y) / 2 - 30 };
          pos = bez(popAt, c, end, u);
          scale = 2.3 - 1.3 * u;
        }
        el.style.transform = `translate(${pos.x - 4}px, ${pos.y - 4}px) scale(${scale})`;
        if (k > 0.12 && k < 0.96) {
          const size = 5 + 3 * Math.min(1, scale - 1);
          if (!lastTrail) { lastTrail = pos; trail(pos, size); }
          else {
            // one spark per ~7px travelled, spread along the way (a fast frame leaves two, not a gap)
            const dx = pos.x - lastTrail.x, dy = pos.y - lastTrail.y, dist = Math.hypot(dx, dy);
            const n = Math.min(3, Math.floor(dist / 7));
            for (let i = 1; i <= n; i++) trail({ x: lastTrail.x + (dx * i) / n, y: lastTrail.y + (dy * i) / n }, size);
            if (n) lastTrail = pos;
          }
        }

        if (k < 1) requestAnimationFrame(frame);
        else { el.remove(); done(); }
      };
      requestAnimationFrame(frame);
    });
  }

  /** A new task: from its row in the conversation to its own chip. */
  function fly(toolId: string, taskId: string | null, total = TOTAL): Promise<void> {
    const from = source(toolId);
    if (!from || !target(taskId) || calm() || document.hidden) {
      land(taskId);
      return Promise.resolve();
    }
    return ember(from, () => target(taskId), total, 40).then(() => land(taskId));
  }

  /** Run a change to the row and slide everything that stays to its new place, instead of jumping. */
  async function reflow(change: () => void) {
    const sel = '[data-bg-chip], [data-bg-stack], [data-bg-tray]';
    const before = new Map([...document.querySelectorAll<HTMLElement>(sel)].map((e) => [e, e.getBoundingClientRect()]));
    change();
    await tick();
    if (calm()) return;
    for (const [el, was] of before) {
      if (!el.isConnected) continue;
      const now = el.getBoundingClientRect();
      const dx = was.left - now.left, dy = was.top - now.top;
      if (Math.abs(dx) < 1 && Math.abs(dy) < 1) continue;
      el.animate([{ transform: `translate(${dx}px, ${dy}px)` }, { transform: 'none' }], { duration: 220, easing: 'cubic-bezier(0.22, 1, 0.36, 1)' });
    }
  }

  /** A task ended and pi's notice for it has reached the conversation: the chip collapses into its own dot, the dot
      flies to the notice bubble, and the bubble appears where it lands. The chip leaves the row the moment it collapses,
      so the others close up at once. */
  async function handoff(id: string): Promise<void> {
    const chip = chipOf(id);
    const rect = chip?.getBoundingClientRect();
    const dot = chip?.querySelector('.dot')?.getBoundingClientRect();
    const bubble = () => document.querySelector<HTMLElement>(`[data-task-notice="${CSS.escape(id)}"]`);
    // the bubble may be out of view (scrolled up): the ember then heads for the edge of the conversation nearest to it
    const at = (): P | null => {
      const r = bubble()?.getBoundingClientRect();
      if (!r || !r.width) return null;
      const sc = document.querySelector('.scroller')?.getBoundingClientRect();
      const y = sc ? Math.min(Math.max(r.top + r.height / 2, sc.top + 22), sc.bottom - 22) : r.top + r.height / 2;
      return { x: r.left + 20, y };
    };
    const arrive = () => {
      chat.landed(id);
      requestAnimationFrame(() => {
        if (calm()) return;
        bubble()?.animate(
          [{ transform: 'scale(0.6)', opacity: 0 }, { transform: 'scale(1.06)', opacity: 1, offset: 0.55 }, { transform: 'scale(1)', opacity: 1 }],
          { duration: 520, easing: 'cubic-bezier(0.3, 1.5, 0.5, 1)' },
        );
      });
    };
    if (!chip || !rect || !dot || !at() || calm() || document.hidden) return arrive();
    const from = mid(dot);
    const ghost = chip.cloneNode(true) as HTMLElement;
    ghost.removeAttribute('data-bg-chip');
    ghost.style.cssText = `position:fixed;left:${rect.left}px;top:${rect.top}px;width:${rect.width}px;height:${rect.height}px;margin:0;pointer-events:none;transform-origin:${from.x - rect.left}px ${from.y - rect.top}px`;
    layer.append(ghost);
    await reflow(() => chat.dismissBg(id));
    const collapse = ghost.animate([{ transform: 'scale(1)', opacity: 1 }, { transform: 'scale(0.06)', opacity: 0 }], { duration: 200, easing: 'ease-in', fill: 'forwards' });
    await collapse.finished.catch(() => {});
    ghost.remove();
    await ember(from, at, 650, 26);
    arrive();
  }

  // Launches fly one after another, each to the chip of its own task, so two never sit on top of each other.
  // With a long line waiting (a script starting ten at once) each trip gets shorter rather than skipped,
  // so the row still fills from the left, in order.
  const queue: { toolId: string; taskId: string | null }[] = [];
  let flying = false;
  async function drain() {
    if (flying) return;
    flying = true;
    while (queue.length) {
      const next = queue.shift()!;
      await fly(next.toolId, next.taskId, Math.max(300, TOTAL - 90 * queue.length));
      await new Promise((r) => setTimeout(r, 90)); // a beat between chips, so they read as one after another
    }
    flying = false;
  }

  let seen = 0;
  $effect(() => {
    for (const l of chat.bgLaunches) {
      if (l.n <= seen) continue;
      seen = l.n;
      queue.push({ toolId: l.toolId, taskId: l.taskId });
    }
    void drain();
  });

  // Notices take their chips one at a time, after the launches have all arrived.
  const handQueue: string[] = [];
  const seenHandoffs = new Set<string>();
  let handing = false;
  async function drainHandoffs() {
    if (handing) return;
    handing = true;
    while (handQueue.length) {
      while (flying || queue.length) await new Promise((r) => setTimeout(r, 120));
      await handoff(handQueue.shift()!);
    }
    handing = false;
  }
  $effect(() => {
    for (const h of chat.bgHandoffs) {
      if (seenHandoffs.has(h.id + ':' + h.n)) continue;
      seenHandoffs.add(h.id + ':' + h.n);
      handQueue.push(h.id);
    }
    void drainHandoffs();
  });

  // Endings: each pops its own chip. A chip still waiting for its ember pops when it has landed.
  let handled = 0;
  $effect(() => {
    const events = chat.bgEvents;
    for (const ev of events) {
      if (ev.n <= handled) continue;
      handled = ev.n;
      const play = () => celebrate(ev.id, ev.status);
      if (chat.bgLanding.includes(ev.id)) setTimeout(play, 900);
      else requestAnimationFrame(play); // after the chip has taken its new colour
    }
  });
</script>

<div class="layer" bind:this={layer} aria-hidden="true"></div>

<style>
  .layer {
    position: fixed;
    inset: 0;
    z-index: 90;
    pointer-events: none;
    overflow: hidden;
    contain: strict;
  }
  .layer :global(.ember) {
    position: absolute;
    left: 0;
    top: 0;
    width: 8px;
    height: 8px;
    border-radius: 2px;
    background: var(--heat);
    box-shadow: 0 0 10px 2px color-mix(in srgb, var(--heat) 70%, transparent);
    will-change: transform;
    backface-visibility: hidden;
  }
  .layer :global(.spark) {
    position: absolute;
    border-radius: 2px;
    background: var(--heat);
    animation: spark 0.42s ease-out forwards;
    will-change: transform, opacity;
  }
  @keyframes spark {
    to {
      opacity: 0;
      transform: scale(0.2);
    }
  }
</style>
