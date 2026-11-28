<script lang="ts">
  import { tick } from 'svelte';
  import { t } from '../lib/i18n.svelte';
  import { api, type UsageRow, type UsageStats } from '../lib/api';
  import { providerColor, providerIcon, providerName } from '../lib/limits';
  import { compactNumber } from '../lib/util';
  import { reducedMotion, springHeights } from '../lib/unroll';
  import Icon from './Icon.svelte';

  // The info tab's "total usage": tokens from every session file on the server, per provider, over a period.
  // Folded to one line by default (total + a small donut); opened, the donut grows into the big one with the
  // providers' cards under it. Read once when shown and when a turn ends; every minute only while open.
  let { running = false }: { running?: boolean } = $props();

  const PERIODS = ['today', 'd7', 'd30', 'all'] as const;
  type Period = (typeof PERIODS)[number];
  const KEY = 'pi-oven.usagePeriod';
  const OPEN_KEY = 'pi-oven.usageOpen';
  const read = (k: string) => { try { return localStorage.getItem(k); } catch { return null; } };
  const saved = read(KEY);
  let period = $state<Period>(PERIODS.find((p) => p === saved) ?? 'd7');
  let open = $state(read(OPEN_KEY) === '1');
  let stats = $state<UsageStats | null>(null);
  let openModels = $state<Record<string, boolean>>({});
  let box = $state<HTMLElement>();
  let busy = false;

  async function load() {
    try { stats = await api.usageStats(new Date().getTimezoneOffset()); } catch { /* keeps what was there */ }
  }
  $effect(() => { void load(); });
  $effect(() => {
    if (!open) return;
    const id = setInterval(() => { if (!document.hidden) void load(); }, 60_000);
    return () => clearInterval(id);
  });
  let wasRunning = false;
  $effect(() => {
    if (wasRunning && !running) void load();
    wasRunning = running;
  });

  function pick(p: Period) {
    period = p;
    try { localStorage.setItem(KEY, p); } catch { /* private mode */ }
  }
  function onKey(e: KeyboardEvent) {
    const d = e.key === 'ArrowRight' ? 1 : e.key === 'ArrowLeft' ? -1 : 0;
    if (!d) return;
    e.preventDefault();
    pick(PERIODS[(PERIODS.indexOf(period) + d + PERIODS.length) % PERIODS.length]);
    queueMicrotask(() => (e.currentTarget as HTMLElement).querySelector<HTMLElement>('[aria-checked="true"]')?.focus());
  }

  const dayName = (d: Date) => `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
  const total = (r: UsageRow) => r.input + r.output + r.cacheRead + r.cacheWrite;
  /** The days shown: the last N local days, today included, or all of them. */
  const span = $derived.by(() => {
    const n = { today: 1, d7: 7, d30: 30, all: 0 }[period];
    if (!n) return stats?.days ?? [];
    const from = new Date();
    from.setDate(from.getDate() - (n - 1));
    const first = dayName(from);
    return (stats?.days ?? []).filter((d) => d.day >= first);
  });
  /** One cell per day: the last 7 or 30 (all time shows the last 30). */
  const cells = $derived.by(() => {
    const n = { today: 1, d7: 7, d30: 30, all: 30 }[period];
    if (n < 2) return null;
    return Array.from({ length: n }, (_, i) => {
      const d = new Date();
      d.setDate(d.getDate() - (n - 1 - i));
      return dayName(d);
    });
  });

  const cards = $derived.by(() => {
    const by = new Map<string, { sum: UsageRow; models: Map<string, UsageRow>; perDay: Map<string, number> }>();
    for (const { day, rows } of span) {
      for (const r of rows) {
        const c = by.get(r.provider) ?? { sum: { ...r, model: '', requests: 0, input: 0, output: 0, cacheRead: 0, cacheWrite: 0, cost: 0 }, models: new Map(), perDay: new Map() };
        by.set(r.provider, c);
        const m = c.models.get(r.model) ?? { ...r, requests: 0, input: 0, output: 0, cacheRead: 0, cacheWrite: 0, cost: 0 };
        c.models.set(r.model, m);
        for (const x of [c.sum, m]) {
          x.requests += r.requests; x.input += r.input; x.output += r.output; x.cacheRead += r.cacheRead; x.cacheWrite += r.cacheWrite; x.cost += r.cost;
        }
        c.perDay.set(day, (c.perDay.get(day) ?? 0) + total(r));
      }
    }
    // the providers without a colour of their own share the grey-blue, each one fainter than the one before
    let others = 0;
    return [...by.entries()]
      .map(([provider, c]) => ({ provider, ...c, total: total(c.sum), models: [...c.models.values()].sort((a, b) => total(b) - total(a)) }))
      .filter((c) => c.total > 0 || c.sum.requests > 0)
      .sort((a, b) => b.total - a.total)
      .map((c) => {
        let color = providerColor(c.provider);
        if (color === 'var(--pv-other)' && others++) color = `color-mix(in srgb, ${color} ${[66, 44][others - 2] ?? 30}%, var(--paper))`;
        return { ...c, color };
      });
  });
  const grand = $derived(cards.reduce((s, c) => s + c.total, 0));
  const cost = $derived(cards.reduce((s, c) => s + c.sum.cost, 0));
  const anyEver = $derived(!!stats?.days.some((d) => d.rows.length));
  const money = (n: number) => `$${n.toFixed(n < 1 ? 3 : 2)}`;
  const pct = (v: number) => (grand ? Math.round((v / grand) * 100) : 0);
  const ratio = (r: UsageRow) => {
    const all = r.input + r.cacheRead + r.cacheWrite;
    return all ? Math.round((r.cacheRead / all) * 100) : 0;
  };
  /** A day's cell: the provider's colour, stronger on a busier day; nothing on a day without use. */
  const cellBg = (v: number, max: number, color: string) => (v ? `color-mix(in srgb, ${color} ${22 + Math.round((v / max) * 78)}%, var(--paper-sunk))` : '');

  /** The donut's arcs in a 20×20 box, one per provider from the top, a small gap between them. */
  function arcs(gapPct: number) {
    let at = 0;
    const gap = cards.length > 1 ? gapPct : 0;
    return cards.map((c) => {
      const share = grand ? (c.total / grand) * 100 : 0;
      const a = { color: c.color, len: Math.max(0.8, share - gap), off: -at - gap / 2 };
      at += share;
      return a;
    });
  }

  // opening: the box grows (spring), the small donut flies to the big one's place and grows into it, the words rise
  const parts = () => box && {
    line: box.querySelector<HTMLElement>('.sum')!,
    mini: box.querySelector<HTMLElement>('.mini'),
    big: box.querySelector<HTMLElement>('.bigd'),
    words: [...box.querySelectorAll<HTMLElement>('.seg, .legend li, .bigd .in, .api, .name, dl, .axis, .more-btn')],
    cells: [...box.querySelectorAll<HTMLElement>('.cells')],
  };
  const fromMini = (mini: HTMLElement, big: HTMLElement) => {
    const a = mini.getBoundingClientRect();
    const b = big.getBoundingClientRect();
    return `translate(${a.left - b.left}px, ${a.top - b.top}px) scale(${a.width / b.width})`;
  };
  const SPRING = 'cubic-bezier(0.3, 1.5, 0.5, 1)';
  const OUT = 'cubic-bezier(0.2, 0.8, 0.2, 1)';

  async function toggle() {
    if (busy || !box) return;
    busy = true;
    try {
      const next = !open;
      try { localStorage.setItem(OPEN_KEY, next ? '1' : '0'); } catch { /* private mode */ }
      if (reducedMotion()) {
        open = next;
        if (next) void load();
        return;
      }
      const from = box.offsetHeight;
      if (next) {
        open = true;
        void load();
        await tick();
        const p = parts();
        if (!p) return;
        box.animate(springHeights(from, box.offsetHeight, 280, 6), { duration: 280, easing: 'linear' });
        if (p.mini && p.big) p.big.animate([{ transform: fromMini(p.mini, p.big) }, { transform: 'none' }], { duration: 460, easing: SPRING });
        p.words.forEach((w, i) =>
          w.animate([{ transform: 'translateY(4px)', opacity: 0 }, { transform: 'none', opacity: 1 }], { duration: 220, delay: 120 + i * 22, easing: 'ease-out', fill: 'backwards' }),
        );
        p.cells.forEach((c, ci) =>
          [...c.children].forEach((b, j) =>
            b.animate([{ transform: 'scale(0)' }, { transform: 'none' }], { duration: 300, delay: 220 + ci * 60 + j * (c.children.length > 7 ? 8 : 26), easing: SPRING, fill: 'backwards' }),
          ),
        );
      } else {
        const p = parts();
        if (!p) return;
        // the words go first, the big donut goes back into the small one, then the box folds (spring, 6 px past)
        const words = [...box.querySelectorAll<HTMLElement>('.more > :not(.whole), .legend, .bigd .in, .api')];
        await Promise.all(words.map((w) => w.animate([{ opacity: 1 }, { opacity: 0 }], { duration: 90, fill: 'forwards' }).finished.catch(() => {})));
        if (p.mini && p.big) {
          await p.big.animate([{ transform: 'none' }, { transform: fromMini(p.mini, p.big) }], { duration: 260, easing: OUT, fill: 'forwards' }).finished.catch(() => {});
          p.big.style.opacity = '0';
        }
        box.classList.remove('open'); // the arrow turns back and the small donut shows while it folds
        await box.animate(springHeights(from, p.line.offsetHeight + 2, 280, 6), { duration: 280, easing: 'linear' }).finished.catch(() => {});
        open = false;
      }
    } finally {
      busy = false;
    }
  }
</script>

{#snippet donut(cls: string, stroke: number, gap: number)}
  {@const r = 10 - stroke / 2 - 0.6}
  <svg class="donut {cls}" viewBox="0 0 20 20" aria-hidden="true">
    <circle cx="10" cy="10" {r} stroke="var(--rule)" stroke-width={stroke} />
    {#each arcs(gap) as a, i (i)}
      <circle cx="10" cy="10" {r} stroke={a.color} stroke-width={stroke} pathLength="100" stroke-dasharray="{a.len} {100 - a.len}" stroke-dashoffset={a.off} transform="rotate(-90 10 10)" />
    {/each}
  </svg>
{/snippet}

{#if stats}
  <section>
    <h3>{t('usage.title')}</h3>
    <div class="fold" class:open bind:this={box}>
      <button type="button" class="sum" aria-expanded={open} disabled={!open && !anyEver} onclick={toggle}>
        <span class="arr"><Icon name="right" size={12} /></span>
        <span class="lbl">{t('usage.total')} <span class="per">· {t(`usage.${period}`)}</span></span>
        <span class="right">
          {#if cards.length}<span class="tot">{compactNumber(grand)}</span>{:else}<span class="none">{t('usage.empty')}</span>{/if}
          {@render donut('mini', 2.4, 6)}
        </span>
      </button>
      {#if open}
        <div class="more">
          <div class="seg" role="radiogroup" tabindex="-1" aria-label={t('usage.period')} onkeydown={onKey}>
            {#each PERIODS as p (p)}
              <button type="button" role="radio" aria-checked={period === p} tabindex={period === p ? 0 : -1} onclick={() => pick(p)}>{t(`usage.${p}`)}</button>
            {/each}
          </div>
          {#if !cards.length}
            <p class="empty">{t('usage.empty')}</p>
          {:else}
            <div class="whole">
              <div class="bigd" role="img" aria-label={t('usage.share')}>
                {@render donut('big', 1.7, 2.2)}
                <div class="in"><b>{compactNumber(grand)}</b><small>{t('usage.tokens')}</small></div>
              </div>
              <ul class="legend">
                {#each cards as c (c.provider)}
                  <li><span class="sw" style:background={c.color}></span><span class="nm">{providerName(c.provider)}</span><span class="pc">{pct(c.total)}%</span></li>
                {/each}
              </ul>
            </div>
            {#if cost > 0}
              <p class="api" title={t('usage.apiCostTip')}>
                <span>{t('usage.apiCost')}<span class="q" aria-hidden="true">?</span></span><b>{money(cost)}</b>
              </p>
            {/if}
            {#each cards as c (c.provider)}
              {@const icon = providerIcon(c.provider)}
              {@const max = Math.max(1, ...(cells ?? []).map((d) => c.perDay.get(d) ?? 0))}
              <div class="prov">
                <div class="name">
                  <span class="sw" style:background={c.color}></span>
                  {#if icon}<Icon name={icon} size={14} />{/if}
                  <b>{providerName(c.provider)}</b>
                  <span class="v">{compactNumber(c.total)}</span>
                </div>
                <dl>
                  <dt>{t('usage.requests')}</dt><dd>{c.sum.requests.toLocaleString()}</dd>
                  <dt>{t('usage.input')}</dt><dd>{c.sum.input.toLocaleString()}</dd>
                  <dt>{t('usage.cacheRead')}</dt><dd>{c.sum.cacheRead.toLocaleString()}</dd>
                  <dt>{t('usage.cacheWrite')}</dt><dd>{c.sum.cacheWrite.toLocaleString()}</dd>
                  <dt>{t('usage.output')}</dt><dd>{c.sum.output.toLocaleString()}</dd>
                  <dt>{t('usage.cacheRatio')}</dt><dd>{ratio(c.sum)}%</dd>
                  <dt>{t('usage.apiCostRow')}</dt>
                  {#if c.sum.cost > 0}<dd>{money(c.sum.cost)}</dd>{:else}<dd class="na">{t('usage.noPrice')}</dd>{/if}
                </dl>
                {#if cells}
                  <div class="cells" class:dense={cells.length > 7} role="img" aria-label={t('usage.perDay')}>
                    {#each cells as d (d)}
                      {@const v = c.perDay.get(d) ?? 0}
                      <i class:on={v > 0} style:background={cellBg(v, max, c.color)} title="{d} · {compactNumber(v)}"></i>
                    {/each}
                  </div>
                  <div class="axis"><span>{t('usage.daysAgo', { n: cells.length - 1 })}</span><span>{t('usage.today')}</span></div>
                {/if}
                <button type="button" class="more-btn" aria-expanded={!!openModels[c.provider]} onclick={() => (openModels[c.provider] = !openModels[c.provider])}>
                  <span class="arr2" class:open={openModels[c.provider]}><Icon name="right" size={12} /></span>{t('usage.models')} {c.models.length}
                </button>
                {#if openModels[c.provider]}
                  <ul class="models">
                    {#each c.models as m (m.model)}
                      <li><span class="mono mname" title={m.model}>{m.model}</span><span class="mono">{compactNumber(total(m))} · {m.requests.toLocaleString()}</span></li>
                    {/each}
                  </ul>
                {/if}
              </div>
            {/each}
          {/if}
        </div>
      {/if}
    </div>
  </section>
{/if}

<style>
  h3 {
    margin: 0 0 8px;
    font-size: 13px;
    font-weight: 700;
    color: var(--ink-2);
  }
  /* folded: one line like a plan line in 구독 한도; opened, the same box grows into the card */
  .fold {
    border: 1px solid var(--rule);
    border-radius: 10px;
    overflow: hidden;
    background: var(--paper);
  }
  .sum {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 9px 12px;
    font-size: 13px;
    text-align: left;
    border-radius: 0;
  }
  .sum:disabled {
    cursor: default;
  }
  @media (hover: hover) {
    .sum:not(:disabled):hover {
      background: var(--paper-sunk);
    }
  }
  .arr,
  .arr2 {
    display: inline-flex;
    flex: none;
    color: var(--ink-3);
    transition: transform 0.16s;
  }
  .fold.open .arr,
  .arr2.open {
    transform: rotate(90deg);
  }
  .lbl {
    white-space: nowrap;
  }
  .per {
    color: var(--ink-3);
    transition: opacity 0.12s;
  }
  .right {
    margin-left: auto;
    display: inline-flex;
    align-items: center;
    gap: 8px;
  }
  .tot {
    font-weight: 700;
    font-family: var(--font-mono);
    font-size: 12.5px;
    font-variant-numeric: tabular-nums;
    transition: opacity 0.12s;
  }
  .none {
    color: var(--ink-3);
  }
  .fold.open .per,
  .fold.open .tot,
  .fold.open .right :global(.mini) {
    opacity: 0;
  }
  .donut {
    display: block;
    flex: none;
    fill: none;
  }
  .right :global(.mini) {
    width: 18px;
    height: 18px;
  }

  .more {
    padding: 2px 12px 12px;
  }
  .seg {
    display: flex;
    width: max-content;
    margin-bottom: 12px;
    border: 1px solid var(--rule);
    border-radius: 8px;
    overflow: hidden;
  }
  .seg button {
    padding: 3px 9px;
    font-size: 12px;
    font-weight: 560;
    color: var(--ink-2);
    border-radius: 0;
  }
  .seg button[aria-checked='true'] {
    background: var(--raised);
    color: var(--ink);
  }
  @media (hover: hover) {
    .seg button:hover {
      background: var(--paper-sunk);
    }
  }
  .empty {
    margin: 0;
    color: var(--ink-3);
    font-size: 12px;
  }

  /* the total: a donut per provider with the sum inside, and its legend */
  .whole {
    display: flex;
    align-items: center;
    gap: 14px;
  }
  .bigd {
    position: relative;
    width: 92px;
    height: 92px;
    flex: none;
    transform-origin: 0 0;
  }
  .bigd :global(.big) {
    width: 92px;
    height: 92px;
  }
  .in {
    position: absolute;
    inset: 0;
    display: grid;
    place-content: center;
    text-align: center;
    line-height: 1.05;
  }
  .in b {
    font-size: 16px;
    font-weight: 750;
    letter-spacing: -0.02em;
    font-variant-numeric: tabular-nums;
  }
  .in small {
    font-size: 10.5px;
    color: var(--ink-3);
  }
  .legend {
    flex: 1;
    min-width: 0;
    display: grid;
    gap: 5px;
    margin: 0;
    padding: 0;
    list-style: none;
    font-size: 12.5px;
  }
  .legend li {
    display: flex;
    align-items: center;
    gap: 7px;
  }
  .nm {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--ink-2);
  }
  .pc {
    font-family: var(--font-mono);
    font-variant-numeric: tabular-nums;
  }
  .sw {
    width: 8px;
    height: 8px;
    flex: none;
    border-radius: 2px;
  }
  /* what the tokens would cost at pay-as-you-go API prices */
  .api {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 8px;
    margin: 12px 0 0;
    padding: 8px 10px;
    border-radius: 8px;
    background: var(--paper-sunk);
    font-size: 12.5px;
    color: var(--ink-2);
    cursor: help;
  }
  .api b {
    color: var(--ink);
    font-family: var(--font-mono);
    font-size: 14px;
    font-variant-numeric: tabular-nums;
  }
  .q {
    display: inline-grid;
    place-content: center;
    width: 14px;
    height: 14px;
    margin-left: 4px;
    border: 1px solid var(--rule);
    border-radius: 50%;
    font-size: 9.5px;
    line-height: 1;
    color: var(--ink-3);
    vertical-align: 1px;
  }

  .prov {
    margin-top: 12px;
    padding-top: 12px;
    border-top: 1px solid var(--rule);
    min-width: 0;
  }
  .name {
    display: flex;
    align-items: center;
    gap: 7px;
    font-size: 13px;
  }
  .v {
    margin-left: auto;
    font-family: var(--font-mono);
    font-weight: 700;
    font-size: 12.5px;
    font-variant-numeric: tabular-nums;
  }
  dl {
    display: grid;
    grid-template-columns: 1fr auto;
    gap: 3px 12px;
    margin: 8px 0 0;
    font-size: 12.5px;
  }
  dt {
    color: var(--ink-2);
  }
  dd {
    margin: 0;
    text-align: right;
    font-family: var(--font-mono);
    font-variant-numeric: tabular-nums;
  }
  dd.na {
    font-family: inherit;
    color: var(--ink-3);
  }
  /* one small rounded square per day, the provider's colour stronger on a busier day */
  .cells {
    display: flex;
    gap: 3px;
    margin-top: 10px;
  }
  .cells i {
    flex: 1 1 0;
    height: 12px;
    border-radius: 2px;
    background: var(--paper-sunk);
    box-shadow: inset 0 0 0 1px var(--rule);
  }
  .cells i.on {
    box-shadow: none;
  }
  .cells.dense {
    gap: 2px;
  }
  .cells.dense i {
    border-radius: 1.5px;
  }
  .axis {
    display: flex;
    justify-content: space-between;
    margin-top: 4px;
    font-size: 10.5px;
    color: var(--ink-3);
  }
  .more-btn {
    display: flex;
    align-items: center;
    gap: 4px;
    margin-top: 8px;
    padding: 2px 0;
    font-size: 12.5px;
    color: var(--ink-2);
  }
  .models {
    list-style: none;
    margin: 6px 0 0;
    padding: 0;
    display: grid;
    gap: 4px;
    font-size: 12px;
  }
  .models li {
    display: flex;
    justify-content: space-between;
    gap: 10px;
    min-width: 0;
  }
  .mname {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--ink-2);
  }
  @media (prefers-reduced-motion: reduce) {
    .arr,
    .arr2,
    .per,
    .tot {
      transition: none;
    }
  }
</style>
