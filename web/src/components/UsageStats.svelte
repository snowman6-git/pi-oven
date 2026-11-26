<script lang="ts">
  import { t } from '../lib/i18n.svelte';
  import { api, type UsageRow, type UsageStats } from '../lib/api';
  import { providerIcon, providerName } from '../lib/limits';
  import { compactNumber } from '../lib/util';
  import Icon from './Icon.svelte';

  // The info tab's "total usage": tokens from every session file on the server, per provider, over a period.
  // `running` turning false (a turn ended) reads it again; so does a minute going by while it is on screen.
  let { running = false }: { running?: boolean } = $props();

  const PERIODS = ['today', 'd7', 'd30', 'all'] as const;
  type Period = (typeof PERIODS)[number];
  const KEY = 'pi-oven.usagePeriod';
  const saved = (() => { try { return localStorage.getItem(KEY); } catch { return null; } })();
  let period = $state<Period>(PERIODS.find((p) => p === saved) ?? 'd7');
  let stats = $state<UsageStats | null>(null);
  let openModels = $state<Record<string, boolean>>({});

  async function load() {
    try { stats = await api.usageStats(new Date().getTimezoneOffset()); } catch { /* keeps what was there */ }
  }
  $effect(() => {
    void load();
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
  const bars = $derived.by(() => {
    const n = { today: 1, d7: 7, d30: 30, all: 0 }[period];
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
    return [...by.entries()]
      .map(([provider, c]) => ({ provider, ...c, total: total(c.sum), models: [...c.models.values()].sort((a, b) => total(b) - total(a)) }))
      .filter((c) => c.total > 0 || c.sum.requests > 0)
      .sort((a, b) => b.total - a.total);
  });
  const grand = $derived(cards.reduce((s, c) => s + c.total, 0));
  const money = (n: number) => `$${n.toFixed(n < 1 ? 3 : 2)}`;
  const ratio = (r: UsageRow) => {
    const all = r.input + r.cacheRead + r.cacheWrite;
    return all ? Math.round((r.cacheRead / all) * 100) : 0;
  };
  /** Bars at most this tall (px) for a card: the busiest day fills it. */
  const barH = (v: number, max: number) => (v ? Math.max(2, Math.round((v / max) * 36)) : 0);
</script>

{#if stats}
  <section>
    <div class="head">
      <h3>{t('usage.title')}</h3>
      <div class="seg" role="radiogroup" tabindex="-1" aria-label={t('usage.period')} onkeydown={onKey}>
        {#each PERIODS as p (p)}
          <button type="button" role="radio" aria-checked={period === p} tabindex={period === p ? 0 : -1} onclick={() => pick(p)}>{t(`usage.${p}`)}</button>
        {/each}
      </div>
    </div>
    {#if !cards.length}
      <p class="empty">{t('usage.empty')}</p>
    {:else}
      {#if cards.length > 1}
        <div class="share" role="img" aria-label={t('usage.share')}>
          {#each cards as c, i (c.provider)}
            <i style="flex:{c.total} 1 0;opacity:{1 - i * 0.28}" title="{providerName(c.provider)} {Math.round((c.total / grand) * 100)}%"></i>
          {/each}
        </div>
      {/if}
      <div class="cards">
        {#each cards as c (c.provider)}
          {@const icon = providerIcon(c.provider)}
          {@const max = Math.max(1, ...(bars ?? []).map((d) => c.perDay.get(d) ?? 0))}
          <div class="card">
            <div class="name">
              {#if icon}<Icon name={icon} size={16} />{/if}
              <b>{providerName(c.provider)}</b>
              {#if grand > 0 && cards.length > 1}<small>{Math.round((c.total / grand) * 100)}%</small>{/if}
            </div>
            <p class="big">{compactNumber(c.total)}</p>
            <p class="sub">
              {t('usage.tokens')}{#if c.sum.cost > 0}{' · '}<span title={t('usage.apiCostTip')}>{t('usage.apiCost', { cost: money(c.sum.cost) })}</span>{/if}
            </p>
            <dl>
              <dt>{t('usage.requests')}</dt><dd>{c.sum.requests.toLocaleString()}</dd>
              <dt>{t('usage.input')}</dt><dd>{c.sum.input.toLocaleString()}</dd>
              <dt>{t('usage.cacheRead')}</dt><dd>{c.sum.cacheRead.toLocaleString()}</dd>
              <dt>{t('usage.cacheWrite')}</dt><dd>{c.sum.cacheWrite.toLocaleString()}</dd>
              <dt>{t('usage.output')}</dt><dd>{c.sum.output.toLocaleString()}</dd>
            </dl>
            <p class="sub">{t('usage.cacheRatio')} {ratio(c.sum)}%</p>
            {#if bars}
              <div class="days" role="img" aria-label={t('usage.perDay')}>
                {#each bars as d (d)}
                  <i style="height:{barH(c.perDay.get(d) ?? 0, max)}px" title="{d} · {compactNumber(c.perDay.get(d) ?? 0)}"></i>
                {/each}
              </div>
            {/if}
            <button type="button" class="more" aria-expanded={!!openModels[c.provider]} onclick={() => (openModels[c.provider] = !openModels[c.provider])}>
              <span class="arr" class:open={openModels[c.provider]}><Icon name="right" size={12} /></span>{t('usage.models')} {c.models.length}
            </button>
            {#if openModels[c.provider]}
              <ul>
                {#each c.models as m (m.model)}
                  <li><span class="mono mname" title={m.model}>{m.model}</span><span class="mono">{compactNumber(total(m))} · {m.requests.toLocaleString()}</span></li>
                {/each}
              </ul>
            {/if}
          </div>
        {/each}
      </div>
    {/if}
  </section>
{/if}

<style>
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    flex-wrap: wrap;
    margin-bottom: 8px;
  }
  h3 {
    margin: 0;
    font-size: 13px;
    font-weight: 700;
    color: var(--ink-2);
  }
  .seg {
    display: flex;
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
  .empty,
  .sub,
  small {
    margin: 4px 0 0;
    color: var(--ink-3);
    font-size: 12px;
  }
  .share {
    display: flex;
    gap: 2px;
    height: 6px;
    margin-bottom: 10px;
  }
  .share i {
    background: var(--accent);
    min-width: 2px;
    border-radius: 2px;
  }
  .cards {
    display: grid;
    gap: 10px;
  }
  .card {
    border: 1px solid var(--rule);
    border-radius: 10px;
    padding: 12px;
    background: var(--paper);
    min-width: 0;
  }
  .name {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 13px;
  }
  .big {
    margin: 8px 0 0;
    font-size: 26px;
    line-height: 1;
    font-weight: 750;
    letter-spacing: -0.03em;
    font-variant-numeric: tabular-nums;
  }
  dl {
    display: grid;
    grid-template-columns: 1fr auto;
    gap: 4px 12px;
    margin: 10px 0 0;
    font-size: 13px;
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
  .days {
    display: flex;
    align-items: flex-end;
    gap: 2px;
    height: 36px;
    margin-top: 10px;
  }
  .days i {
    flex: 1 1 0;
    min-width: 1px;
    background: var(--ink-3);
    border-radius: 1px 1px 0 0;
  }
  .more {
    display: flex;
    align-items: center;
    gap: 4px;
    margin-top: 10px;
    padding: 2px 0;
    font-size: 12.5px;
    color: var(--ink-2);
  }
  .arr {
    display: inline-flex;
    transition: transform 0.15s;
  }
  .arr.open {
    transform: rotate(90deg);
  }
  ul {
    list-style: none;
    margin: 6px 0 0;
    padding: 0;
    display: grid;
    gap: 4px;
    font-size: 12px;
  }
  li {
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
    .arr {
      transition: none;
    }
  }
</style>
