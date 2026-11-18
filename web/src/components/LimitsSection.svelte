<script lang="ts">
  import { tick } from 'svelte';
  import { t } from '../lib/i18n.svelte';
  import { limits } from '../lib/limits.svelte';
  import { settings } from '../lib/settings.svelte';
  import type { Limit } from '../lib/limits';
  import Icon from './Icon.svelte';
  import { api } from '../lib/api';
  import { app } from '../lib/app.svelte';
  import LimitRing from './LimitRing.svelte';
  import { creditGroups, halves, planName, providerIcon, providerName, resetDate, until, windowName, type WindowNow } from '../lib/limits';
  import { arcPoints, barPoints, play, reducedMotion, ROLL_UP, springHeights, UNROLL, type Item } from '../lib/unroll';

  // The info tab's "plan limits". The plan behind the chosen model is a card at the top; every other plan is one line
  // with its split ring, opened by a tap. Opening unrolls the ring into the rows' bars; folding rolls them back.
  // When the model changes to another plan, the old card folds, the new plan's line lifts over it into first place
  // ("overhead swap"), and opens there.
  let { provider }: { provider?: string } = $props();

  $effect(() => {
    limits.ensure();
    void limits.refresh(true); // looking at the cards: ask now
  });

  /** The plan shown as in use: follows `provider`, but only once the swap has run. */
  let shown = $state('');
  /** Which plans are open (the one in use always is). */
  let open = $state<Record<string, boolean>>({});
  /** Folding now: the "use" pill goes at once, not after the ring is back (it would sit over it). */
  let closing = $state<Record<string, boolean>>({});
  const folds: Record<string, HTMLElement> = {};

  // A card per plan, and per saved account of it (Settings → Accounts) unless the copies are hidden (Settings → Theme):
  // then one card per provider, whose account changes in place.
  const key = (l: Limit) => (settings.hideDupAccounts || !l.account ? l.provider : `${l.provider}#${l.account.id}`);
  const all = $derived([...Object.values(limits.items), ...(settings.hideDupAccounts ? [] : limits.others)]);
  /** Providers with more than one saved account: their cards say which account. */
  const multi = $derived(new Set(limits.others.map((o) => o.provider)));
  const cards = $derived(
    all
      .map((l) => ({ l, id: key(l) }))
      .sort((a, b) => Number(b.id === shown) - Number(a.id === shown) || a.l.provider.localeCompare(b.l.provider) || Number(!!limits.items[b.l.provider] && limits.items[b.l.provider] === b.l) - Number(!!limits.items[a.l.provider] && limits.items[a.l.provider] === a.l))
      .map(({ l, id }) => {
        const { short, long } = halves(l, limits.now);
        return { l, id, short, long, rows: [short, long].filter((w): w is WindowNow => !!w) };
      }),
  );
  const resetText = (w: WindowNow) =>
    w.resetAt == null ? '' : w.minutes > 1440 ? resetDate(w.resetAt) : t('limits.resetIn', { time: until(w.resetAt, limits.now) });
  const ringTip = (rows: WindowNow[]) => rows.map((w) => `${windowName(w.minutes)} ${t('limits.leftPct', { n: Math.round(w.left) })}`).join(' · ');

  // Another saved account of a provider (not the one in use): its open card has a small "use" pill where the ring was,
  // and switching to it plays the overhead swap like a model change does (Settings → Accounts does the same).
  const isOther = (l: Limit) => !!l.account && limits.items[l.provider]?.account?.id !== l.account.id;
  let switching = $state<string | null>(null);
  async function useAccount(l: Limit) {
    const a = l.account;
    if (!a || switching) return;
    switching = a.id;
    try {
      await api.switchAccount(a.id);
      limits.swapped(l.provider, a.email ?? a.id);
    } catch (e) {
      app.flash(t('limits.switchFailed', { why: (e as Error).message }));
    } finally {
      switching = null;
    }
  }

  // one motion at a time, in the order asked
  let chain: Promise<void> = Promise.resolve();
  const queue = (f: () => Promise<void>) => (chain = chain.then(f).catch(() => {}));

  function parts(id: string) {
    const f = folds[id];
    if (!f?.isConnected) return null;
    const line = f.querySelector<HTMLElement>('.sum')!;
    const ring = f.querySelector<SVGElement>('.ringbox svg');
    const meters = [...f.querySelectorAll<HTMLElement>('.meter[data-side]')];
    const words = [...f.querySelectorAll<HTMLElement>('.more .label, .more .row > small, .more .credits')];
    return { f, line, ring, meters, words };
  }
  const itemsOf = (p: NonNullable<ReturnType<typeof parts>>): Item[] =>
    p.ring ? p.meters.map((m) => ({ arc: arcPoints(p.ring!, m.dataset.side as 'short' | 'long'), bar: barPoints(m), left: Number(m.dataset.left) })) : [];

  async function expand(id: string) {
    const p0 = parts(id);
    if (!p0 || open[id]) {
      open[id] = true;
      return;
    }
    const from = p0.f.offsetHeight;
    open[id] = true;
    await tick();
    const p = parts(id);
    if (!p || reducedMotion()) return;
    const to = p.f.offsetHeight;
    const items = itemsOf(p);
    for (const m of p.meters) m.style.visibility = 'hidden';
    p.f.animate(springHeights(from, to, 280, 12), { duration: 280, easing: 'linear' });
    p.words.forEach((w, i) =>
      w.animate([{ opacity: 0, transform: 'translateY(4px)' }, { opacity: 1, transform: 'none' }], {
        duration: 240,
        delay: 90 + i * 45,
        easing: 'cubic-bezier(0.2, 0.8, 0.2, 1)',
        fill: 'backwards',
      }),
    );
    await play(items, UNROLL, p.f);
    for (const m of p.meters) m.style.visibility = '';
  }

  async function collapse(id: string) {
    const p = parts(id);
    if (!p || !open[id] || reducedMotion()) {
      open[id] = false;
      return;
    }
    closing[id] = true;
    const from = p.f.offsetHeight;
    const to = p.line.offsetHeight + 2; // the line and the card's border
    const items = itemsOf(p);
    for (const m of p.meters) m.style.visibility = 'hidden';
    // the words go first; the bars shrink and set off for the ring; only then does the card fold (spring, 6 px past)
    for (const w of p.words) w.animate([{ opacity: 1 }, { opacity: 0 }], { duration: 110, easing: 'linear', fill: 'forwards' });
    const fold = p.f.animate(springHeights(from, to, 280, 6), { duration: 280, delay: 230, easing: 'linear', fill: 'backwards' });
    await play(items, ROLL_UP, p.f);
    if (p.ring) p.ring.style.opacity = '1'; // the halves are home: the ring shows while the card folds round it
    await fold.finished.catch(() => {});
    open[id] = false;
    closing[id] = false;
    await tick();
    if (p.ring) p.ring.style.opacity = '';
    for (const w of p.words) w.getAnimations().forEach((a) => a.cancel());
    for (const m of p.meters) m.style.visibility = '';
  }

  /** The model moved to another plan: fold the old card, lift the new line over it into first place, open it. */
  async function swapTo(id: string) {
    if (id === shown) return;
    if (!shown) {
      // the first time: nothing to swap from
      shown = id;
      open[id] = true;
      return;
    }
    await Promise.all(Object.keys(open).filter((k) => open[k]).map(collapse));
    const before = new Map(Object.entries(folds).filter(([, f]) => f?.isConnected).map(([k, f]) => [k, f.getBoundingClientRect().top]));
    shown = id;
    await tick();
    if (!reducedMotion()) {
      await Promise.all(
        Object.entries(folds).map(([k, f]) => {
          const was = before.get(k);
          if (was == null || !f?.isConnected) return;
          const dy = was - f.getBoundingClientRect().top;
          if (!dy) return;
          const up = k === id;
          f.style.zIndex = up ? '2' : '1';
          // going up: a touch larger with a shadow, passing over; going down: a touch smaller, slipping under
          const mid: Keyframe = up
            ? { transform: `translateY(${dy / 2}px) scale(1.04)`, boxShadow: 'var(--shadow)', opacity: 1, offset: 0.45 }
            : { transform: `translateY(${dy / 2}px) scale(0.96)`, boxShadow: 'none', opacity: 0.75, offset: 0.45 };
          const a = f.animate(
            [{ transform: `translateY(${dy}px)`, boxShadow: 'none', opacity: 1 }, mid, { transform: 'none', boxShadow: 'none', opacity: 1 }],
            { duration: 460, easing: 'cubic-bezier(0.3, 1.2, 0.5, 1)' },
          );
          return a.finished.catch(() => {}).then(() => (f.style.zIndex = ''));
        }),
      );
    }
    await expand(id); // a plan whose numbers have not come yet just opens when they do
  }

  /** The card in use: the chosen model's provider, with the account its numbers belong to. */
  const target = $derived.by(() => {
    const p = provider ?? '';
    const l = p ? limits.items[p] : undefined;
    return l ? key(l) : p;
  });
  $effect(() => {
    const k = target;
    if (k && k !== shown) queue(() => swapTo(k));
  });

  // The account behind a provider changed and the copies are hidden: the card stays, its account name slides over
  // and its bars empty and fill again to the new account's numbers ("refill").
  let swapSeen = 0;
  $effect(() => {
    const sw = limits.swap;
    if (!sw || sw.n <= swapSeen) return;
    swapSeen = sw.n;
    if (!settings.hideDupAccounts) return; // the cards trade places instead (the key changes)
    const before = limits.items[sw.provider]?.account?.id;
    queue(() => refill(sw.provider, before));
  });
  async function refill(p: string, before: string | undefined) {
    const f = folds[p];
    if (!f?.isConnected || reducedMotion()) return;
    const bars = [...f.querySelectorAll<HTMLElement>('.meter > i')];
    const name = f.querySelector<HTMLElement>('.acct');
    name?.animate([{ transform: 'none', opacity: 1 }, { transform: 'translateY(-8px)', opacity: 0 }], { duration: 160, easing: 'ease-in', fill: 'forwards' });
    const drains = bars.map((b, i) => b.animate([{ transform: b.style.transform }, { transform: 'scaleX(0)' }], { duration: 260, delay: i * 50, easing: 'cubic-bezier(0.4, 0, 1, 1)', fill: 'forwards' }));
    await Promise.all(drains.map((a) => a.finished.catch(() => {})));
    // the new account's numbers: wait for them a little (the switch asks for them)
    for (let i = 0; i < 25 && limits.items[p]?.account?.id === before; i++) await new Promise((r) => setTimeout(r, 120));
    await tick();
    const now = [...(folds[p]?.querySelectorAll<HTMLElement>('.meter > i') ?? [])];
    drains.forEach((a) => a.cancel());
    name?.getAnimations().forEach((a) => a.cancel());
    folds[p]?.querySelector<HTMLElement>('.acct')?.animate([{ transform: 'translateY(8px)', opacity: 0 }, { transform: 'none', opacity: 1 }], { duration: 300, easing: 'cubic-bezier(0.3, 1.5, 0.5, 1)' });
    await Promise.all(
      now.map((b, i) => b.animate([{ transform: 'scaleX(0)' }, { transform: b.style.transform }], { duration: 520, delay: 60 + i * 60, easing: 'cubic-bezier(0.2, 0.8, 0.2, 1)', fill: 'backwards' }).finished.catch(() => {})),
    );
  }

  function toggle(id: string) {
    if (id === shown) return;
    queue(() => (open[id] ? collapse(id) : expand(id)));
  }
</script>

{#if cards.length}
  <section>
    <h3>{t('limits.title')}</h3>
    <div class="cards">
      {#each cards as { l, id, short, long, rows } (id)}
        {@const isOn = id === shown}
        <div class="fold" class:open={open[id]} class:on={isOn} class:usable={open[id] && isOther(l)} bind:this={folds[id]}>
          <button class="sum" type="button" aria-expanded={!!open[id]} aria-disabled={isOn} onclick={() => toggle(id)}>
            <span class="arr" aria-hidden="true"><Icon name="right" size={12} /></span>
            {#if providerIcon(l.provider)}{@const icon = providerIcon(l.provider)!}<span class="prov" aria-hidden="true"><Icon name={icon} size={16} /></span>{/if}
            <span class="who">{providerName(l.provider)} <small>{planName(l.account?.plan ?? l.plan)}</small>{#if isOn}<em>{t('limits.inUse')}</em>{/if}</span>
            {#if multi.has(l.provider) && l.account?.email}<span class="acct" data-priv="user">{l.account.email}</span>{/if}
            <span class="ringbox" title={ringTip(rows)}><LimitRing {short} {long} size={18} /></span>
          </button>
          {#if open[id] && !closing[id] && isOther(l)}
            {@const full = rows.some((w) => w.reached) || l.limitReached}
            <button class="use" type="button" disabled={full || switching !== null}
              aria-label={t('limits.useLabel', { who: l.account?.email ?? '' })} onclick={() => useAccount(l)}>
              {full ? t('limits.useFull') : switching === l.account?.id ? '…' : t('limits.use')}
            </button>
          {/if}
          <div class="more" hidden={!open[id]}>
            {#each rows as w (w.minutes)}
              <div class="row" class:full={w.reached}>
                <div class="label">
                  <span>{windowName(w.minutes)}{#if w.reached} · {t('limits.reached')}{/if}</span>
                  <span class="num">{t('limits.leftPct', { n: Math.round(w.left) })}</span>
                </div>
                <div class="meter" role="meter" aria-valuenow={Math.round(w.left)} aria-valuemin="0" aria-valuemax="100" aria-label="{providerName(l.provider)} {windowName(w.minutes)}"
                  data-side={w === short ? 'short' : 'long'} data-left={w.left / 100}>
                  <i class={w.level} style="transform:scaleX({w.left / 100})"></i>
                </div>
                {#if w.resetAt != null}<small>{resetText(w)}</small>{/if}
              </div>
            {/each}
            {#if l.resets != null}
              <div class="credits">
                <small>{t('limits.resetCredits', { n: l.resets })}</small>
                {#each creditGroups(l.resetKinds) as g (g.kind + (g.model ?? ''))}
                  <span class="credit" title={g.model ? t('limits.creditModel', { model: g.model }) : t(`limits.credit.${g.kind === 'other' ? 'any' : (g.kind as '5h' | 'week')}`)}>
                    <Icon name={g.icon} size={14} /><b>{g.n}</b>
                  </span>
                {/each}
              </div>
            {/if}
          </div>
        </div>
      {/each}
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
  small {
    color: var(--ink-3);
    font-size: 12px;
  }
  /* which account a card is, when the provider has more than one */
  .acct {
    flex: 1 1 auto;
    text-align: right;
    min-width: 0;
    margin-left: auto;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--font-mono);
    font-size: 11.5px;
    color: var(--ink-3);
  }
  .acct + .ringbox {
    margin-left: 0;
  }
  .cards {
    display: grid;
    gap: 10px;
  }
  /* one plan: a line that grows into its card */
  .fold {
    position: relative;
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
  .fold.on .sum {
    cursor: default;
  }
  @media (hover: hover) {
    .fold:not(.on) .sum:hover {
      background: var(--paper-sunk);
    }
  }
  .arr {
    display: inline-flex;
    width: 12px;
    flex: none;
    overflow: hidden;
    color: var(--ink-3);
    transition: transform 0.16s, opacity 0.16s, width 0.16s, margin 0.16s;
  }
  .fold.open .arr {
    transform: rotate(90deg);
  }
  /* the plan in use cannot be folded: no arrow, and no gap where it was (the icon sits at the edge) */
  .fold.on .arr {
    opacity: 0;
    width: 0;
    margin-right: -8px;
  }
  .prov {
    display: inline-flex;
    color: var(--ink-2);
    margin-right: -2px;
  }
  /* the name, plan and "in use" keep to one line; an account's address gives way first */
  .who {
    flex: none;
    white-space: nowrap;
  }
  .who em {
    margin-left: 6px;
    font-style: normal;
    font-size: 11px;
    font-weight: 600;
    color: var(--accent);
  }
  .ringbox {
    margin-left: auto;
    display: inline-flex;
  }
  /* another account's open card: a small pill where the ring was (the ring has become the bars) */
  .use {
    position: absolute;
    top: 7px;
    right: 10px;
    height: 24px;
    padding: 0 11px;
    border: 1px solid var(--accent);
    border-radius: 999px;
    background: var(--paper);
    color: var(--accent);
    font-size: 12px;
    font-weight: 600;
  }
  .use:disabled {
    border-color: var(--rule);
    color: var(--ink-3);
  }
  @media (hover: hover) {
    .use:not(:disabled):hover {
      background: var(--accent-wash);
    }
  }
  /* the address gives the pill its room (kept while folding, so the text does not jump when the pill goes) */
  .fold.usable .acct {
    padding-right: 52px;
  }
  /* open: the ring has become the bars */
  .fold.open .ringbox :global(svg) {
    opacity: 0;
  }
  .more {
    display: grid;
    gap: 10px;
    padding: 2px 12px 12px;
  }
  .more[hidden] {
    display: none;
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
  /* full width, scaled: a change moves on the compositor instead of laying the bar out every frame */
  .meter i {
    display: block;
    height: 100%;
    background: var(--accent);
    transform-origin: left;
    transition: transform 0.4s;
  }
  .meter i.warm {
    background: var(--heat);
  }
  .meter i.full {
    background: var(--bad);
  }
  .credits {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px 10px;
  }
  .credit {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    color: var(--ink-2);
    font-size: 12px;
  }
  .credit b {
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }
  /* the lines on their way between ring and bars (lib/unroll.ts) */
  :global(.unroll-stage) {
    position: fixed;
    inset: 0;
    width: 100vw;
    height: 100vh;
    pointer-events: none;
    z-index: 50;
    overflow: visible;
  }
  :global(.unroll-stage polyline) {
    fill: none;
    stroke-linecap: butt;
    stroke-linejoin: round;
  }
  :global(.unroll-stage .trk) {
    stroke: var(--rule);
  }
  :global(.unroll-stage .arc) {
    stroke: var(--accent);
  }
  @media (prefers-reduced-motion: reduce) {
    .meter i,
    .arr {
      transition: none;
    }
  }
</style>
