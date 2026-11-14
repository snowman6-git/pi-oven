<script lang="ts">
  import { t } from '../lib/i18n.svelte';
  import { limits } from '../lib/limits.svelte';
  import { halves, planName, providerName, resetDate, until, windowName, type WindowNow } from '../lib/limits';
  import { relativeTime } from '../lib/util';

  // The info tab's "plan limits": one card per subscription that has sent its numbers, the model's own first.
  let { provider }: { provider?: string } = $props();

  $effect(() => {
    limits.ensure();
    void limits.refresh(true); // looking at the cards: ask now
  });
  const cards = $derived(
    Object.values(limits.items)
      .sort((a, b) => Number(b.provider === provider) - Number(a.provider === provider) || a.provider.localeCompare(b.provider))
      .map((l) => {
        const { short, long } = halves(l, limits.now);
        return { l, rows: [short, long].filter((w): w is WindowNow => !!w) };
      }),
  );
  const newest = $derived(Math.max(0, ...cards.map((c) => c.l.at)));
  const resetText = (w: WindowNow) =>
    w.resetAt == null ? '' : w.minutes > 1440 ? resetDate(w.resetAt) : t('limits.resetIn', { time: until(w.resetAt, limits.now) });
</script>

{#if cards.length}
  <section>
    <div class="head">
      <h3>{t('limits.title')}</h3>
      <small>{t('limits.asOf', { time: relativeTime(newest) })}</small>
    </div>
    <div class="cards">
      {#each cards as { l, rows } (l.provider)}
        <div class="card">
          <div class="name">
            <span>{providerName(l.provider)}</span>
            <small>{planName(l.plan)}{#if l.provider === provider}{l.plan ? ' · ' : ''}{t('limits.inUse')}{/if}</small>
          </div>
          {#each rows as w (w.minutes)}
            <div class="row" class:full={w.reached}>
              <div class="label">
                <span>{windowName(w.minutes)}{#if w.reached} · {t('limits.reached')}{/if}</span>
                <span class="num">{t('limits.leftPct', { n: Math.round(w.left) })}</span>
              </div>
              <div class="meter" role="meter" aria-valuenow={Math.round(w.left)} aria-valuemin="0" aria-valuemax="100" aria-label="{providerName(l.provider)} {windowName(w.minutes)}">
                <i class={w.level} style="width:{w.left}%"></i>
              </div>
              {#if w.resetAt != null}<small>{resetText(w)}</small>{/if}
            </div>
          {/each}
          {#if l.resets != null}<small>{t('limits.resetCredits', { n: l.resets })}</small>{/if}
          {#if l.at < newest - 60 * 60_000}<small class="old">{t('limits.asOf', { time: relativeTime(l.at) })}</small>{/if}
        </div>
      {/each}
    </div>
  </section>
{/if}

<style>
  .head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 8px;
    margin-bottom: 8px;
  }
  h3 {
    margin: 0;
    font-size: 13px;
    font-weight: 700;
    color: var(--ink-2);
  }
  small {
    color: var(--ink-3);
    font-size: 12px;
  }
  .cards {
    display: grid;
    gap: 10px;
  }
  .card {
    display: grid;
    gap: 10px;
    padding: 12px;
    border: 1px solid var(--rule);
    border-radius: 10px;
  }
  .name {
    display: flex;
    justify-content: space-between;
    gap: 8px;
    font-size: 13px;
  }
  .row {
    display: grid;
    gap: 5px;
  }
  .label {
    display: flex;
    justify-content: space-between;
    font-size: 12px;
    color: var(--ink-2);
  }
  .num {
    font-variant-numeric: tabular-nums;
  }
  .row.full .label {
    color: var(--bad);
  }
  .meter {
    height: 6px;
    border-radius: 3px;
    background: var(--rule);
    overflow: hidden;
  }
  .meter i {
    display: block;
    height: 100%;
    background: var(--accent);
    transition: width 0.4s;
  }
  .meter i.warm {
    background: var(--heat);
  }
  .meter i.full {
    background: var(--bad);
  }
  @media (prefers-reduced-motion: reduce) {
    .meter i {
      transition: none;
    }
  }
</style>
