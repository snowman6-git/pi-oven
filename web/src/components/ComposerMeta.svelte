<script lang="ts">
  import { grow, shrink } from '../lib/sheet';
  import { t } from '../lib/i18n.svelte';
  import { app } from '../lib/app.svelte';
  import type { Chat, Permission } from '../lib/chat.svelte';
  import { dismiss } from '../lib/actions';
  import type { ModelInfo, ThinkingLevel } from '../lib/protocol';
  import { S } from '../lib/strings';
  import { compactNumber } from '../lib/util';
  import ContextRing from './ContextRing.svelte';
  import LimitRing from './LimitRing.svelte';
  import { settings } from '../lib/settings.svelte';
  import { limits } from '../lib/limits.svelte';
  import { halves, planName, providerName, resetDate, until, untilShort, windowName, type WindowNow } from '../lib/limits';
  import Icon from './Icon.svelte';

  type Menu = 'plus' | 'perm' | 'model' | 'think' | 'ctx' | 'limit' | null;

  // The row under the message box: permission, model, thinking level, context. `menu` is shared with the attach menu
  // in Composer so that only one is open at a time.
  let { chat, menu = $bindable(), toggle }: { chat: Chat; menu: Menu; toggle: (m: Exclude<Menu, null>) => void } = $props();

  let filter = $state('');
  $effect(() => {
    menu;
    filter = '';
  });

  const models = $derived(
    chat.models.filter((m) => `${m.provider} ${m.id} ${m.name ?? ''}`.toLowerCase().includes(filter.toLowerCase())),
  );
  const model = $derived(chat.state?.model);
  const sameModel = (m: ModelInfo) => m.id === model?.id && m.provider === model?.provider;
  const ctx = $derived(chat.stats?.contextUsage);
  const pct = $derived(chat.contextPercent);

  // The subscription behind the chosen model, when the provider has sent its limits (none for local or API-key models).
  $effect(() => limits.ensure());
  const lim = $derived(model ? limits.items[model.provider] : undefined);
  const half = $derived(lim ? halves(lim, limits.now) : {});
  const used = $derived.by(() => {
    const { short, long } = half;
    const full = short?.reached ? short : long?.reached ? long : undefined;
    if (full) return { text: full.resetAt ? untilShort(full.resetAt, limits.now) : t('limits.reached'), level: 'full' };
    const w = short ?? long;
    return w ? { text: `${Math.round(w.left)}%`, level: w.level } : null;
  });
  const resetText = (w: WindowNow) =>
    w.resetAt == null ? '' : w.minutes > 1440 ? t('limits.resetAt', { date: resetDate(w.resetAt, false) }) : t('limits.resetIn', { time: until(w.resetAt, limits.now) });

  const perms: { id: Permission }[] = [{ id: 'auto' }, { id: 'manual' }];
  const permLabel = $derived(t(`meta.perm.${chat.permission}.label`));
</script>

<div class="meta">
  <div class="pop" use:dismiss={() => { if (menu === 'perm') menu = null; }}>
    {#if chat.hasPermissionExt}
      <button class="chip" class:manual={chat.permission === 'manual'} type="button" aria-haspopup="menu" aria-expanded={menu === 'perm'} onclick={() => toggle('perm')}>
        <Icon name="shield" size={14} />{permLabel}<Icon name="down" size={12} />
      </button>
      {#if menu === 'perm'}
        <div class="sheet" in:grow out:shrink role="menu">
          {#each perms as p (p.id)}
            <button role="menuitemradio" aria-checked={chat.permission === p.id} type="button" class:on={chat.permission === p.id}
              onclick={() => ((menu = null), chat.setPermission(p.id))}>
              <span class="optlabel"><b>{t(`meta.perm.${p.id}.label`)}</b><small>{t(`meta.perm.${p.id}.hint`)}</small></span>
              {#if chat.permission === p.id}<Icon name="check" size={16} />{/if}
            </button>
          {/each}
        </div>
      {/if}
    {/if}
  </div>

  <span class="grow"></span>

  <div class="pop" use:dismiss={() => { if (menu === 'model') menu = null; }}>
    <button class="chip" type="button" aria-haspopup="listbox" aria-expanded={menu === 'model'} onclick={() => toggle('model')}>
      <span class="mname">{model?.name ?? model?.id ?? S.model}</span><Icon name="down" size={12} />
    </button>
    {#if menu === 'model'}
      <div class="sheet wide" in:grow out:shrink>
        {#if chat.models.length > 7}
          <input class="field" placeholder={t('meta.search')} bind:value={filter} />
        {/if}
        <ul role="listbox" aria-label={S.model}>
          {#each models as m (m.provider + m.id)}
            <li role="option" aria-selected={sameModel(m)}>
              <button type="button" class:on={sameModel(m)} onclick={() => ((menu = null), chat.setModel(m))}>
                <span>{m.name ?? m.id}</span><small>{m.provider}</small>
              </button>
            </li>
          {:else}
            <li class="none">{t('meta.none')}</li>
          {/each}
        </ul>
        <button class="addmodel" type="button" onclick={() => ((menu = null), app.openSettings('models'))}><Icon name="plus" size={14} />{t('meta.add')}</button>
      </div>
    {/if}
  </div>

  {#if chat.levels.length > 1 && model?.reasoning !== false}
    <div class="pop" use:dismiss={() => { if (menu === 'think') menu = null; }}>
      <button class="chip" type="button" title={S.thinkingLevel} aria-haspopup="listbox" aria-expanded={menu === 'think'} onclick={() => toggle('think')}>
        {chat.state?.thinkingLevel}<Icon name="down" size={12} />
      </button>
      {#if menu === 'think'}
        <div class="sheet narrow" in:grow out:shrink>
          <ul role="listbox" aria-label={S.thinkingLevel}>
            {#each chat.levels as l}
              <li role="option" aria-selected={l === chat.state?.thinkingLevel}>
                <button type="button" class:on={l === chat.state?.thinkingLevel} onclick={() => ((menu = null), chat.setThinking(l as ThinkingLevel))}>{l}</button>
              </li>
            {/each}
          </ul>
        </div>
      {/if}
    </div>
  {/if}

  {#if lim && used && settings.metaLimits}
    <div class="pop" use:dismiss={() => { if (menu === 'limit') menu = null; }}>
      <button class="chip limit {used.level}" type="button" aria-label={t('limits.chip', { text: used.text })} aria-expanded={menu === 'limit'} onclick={() => { if (menu !== 'limit') void limits.refresh(true); toggle('limit'); }}>
        <LimitRing short={half.short} long={half.long} />{#if settings.metaLimitText}{used.text}{/if}
      </button>
      {#if menu === 'limit'}
        <div class="sheet limitsheet" in:grow out:shrink>
          <b>{t('limits.title')} <small>· {providerName(lim.provider)} {planName(lim.plan)}</small></b>
          <div class="halves">
            {#each [half.long, half.short] as w, i (i)}
              {#if w}
                <div class:full={w.reached}>
                  <small>{t('limits.leftOf', { name: windowName(w.minutes) })}</small>
                  <span class="pctbig">{w.reached ? t('limits.reached') : `${Math.round(w.left)}%`}</span>
                  <small>{resetText(w)}</small>
                </div>
              {/if}
            {/each}
          </div>
          <button class="btn" type="button" onclick={() => ((menu = null), app.openInspector('info'))}>{t('limits.showInfo')}</button>
        </div>
      {/if}
    </div>
  {/if}

  {#if settings.metaContext}
  <div class="pop" use:dismiss={() => { if (menu === 'ctx') menu = null; }}>
    <button class="chip ring" type="button" aria-label="{S.context} {pct != null ? Math.round(pct) + '%' : ''}" aria-expanded={menu === 'ctx'} onclick={() => toggle('ctx')}>
      <ContextRing percent={pct} />
    </button>
    {#if menu === 'ctx'}
      <div class="sheet ctxsheet" in:grow out:shrink>
        <b>{S.context}</b>
        {#if ctx && pct != null}
          <span class="pctbig">{Math.round(pct)}%</span>
          <small>{compactNumber(ctx.tokens ?? 0)} / {compactNumber(ctx.contextWindow)} {S.tokens}</small>
        {:else}
          <small>{ctx ? t('inspector.afterCompact') : t('inspector.noModel')}</small>
        {/if}
        <button class="btn" type="button" disabled={chat.running || chat.compacting} onclick={() => ((menu = null), chat.compact())}>{S.compact}</button>
      </div>
    {/if}
  </div>
  {/if}
</div>

<style>
  .meta {
    max-width: calc(var(--measure) + 74px);
    margin-inline: auto;
  }
  .meta {
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 6px 4px 0;
    font-size: 13px;
  }
  .grow {
    flex: 1;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    height: 28px;
    padding: 0 8px;
    border-radius: 8px;
    color: var(--ink-2);
    font-weight: 520;
  }
  @media (hover: hover) {
    .chip:hover,
    .chip[aria-expanded='true'] {
      background: var(--paper-sunk);
      color: var(--ink);
    }
  }
  .chip.manual {
    color: var(--heat);
  }
  .chip.ring {
    padding: 0 6px;
  }
  .mname {
    max-width: 180px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .optlabel {
    display: grid;
    line-height: 1.35;
  }
  .ctxsheet {
    gap: 4px;
    padding: 12px;
  }
  .pctbig {
    font-size: 28px;
    font-weight: 750;
    letter-spacing: -0.03em;
    line-height: 1.1;
  }
  .chip.limit {
    gap: 5px;
    padding: 0 6px;
    font-variant-numeric: tabular-nums;
  }
  .chip.limit.warm {
    color: var(--heat);
  }
  .chip.limit.full {
    color: var(--bad);
  }
  .limitsheet {
    gap: 8px;
    padding: 12px;
  }
  .limitsheet b small {
    font-weight: 400;
    color: var(--ink-3);
  }
  .halves {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
  .halves > div {
    display: grid;
    gap: 2px;
    align-content: start;
  }
  .halves > div + div {
    border-left: 1px solid var(--rule);
    padding-left: 10px;
  }
  .halves > div:first-child:not(:only-child) {
    padding-right: 10px;
  }
  .halves .pctbig {
    font-size: 24px;
  }
  .halves .full .pctbig {
    color: var(--bad);
    font-size: 17px;
    line-height: 1.6;
  }
  .limitsheet small {
    color: var(--ink-3);
  }
  .limitsheet .btn {
    margin-top: 4px;
    justify-content: center;
  }
  .ctxsheet .btn {
    margin-top: 8px;
    justify-content: center;
  }
  @media (max-width: 720px) {
    .mname {
      max-width: 110px;
    }
  }
  @media (max-width: 360px) {
    .mname {
      max-width: 72px;
    }
  }
</style>
