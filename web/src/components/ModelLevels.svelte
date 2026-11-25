<script lang="ts">
  import { t } from '../lib/i18n.svelte';
  import { api, type ModelPrefs } from '../lib/api';
  import { app } from '../lib/app.svelte';
  import { providerName } from '../lib/limits';
  import type { Chat } from '../lib/chat.svelte';
  import type { ModelInfo, ThinkingLevel } from '../lib/protocol';
  import Icon from './Icon.svelte';

  // Settings → Models: the open session's models by provider, each provider a box that folds. A provider's switch
  // takes all its models out of the model menu at once (their own switches are kept for when it is back on); each
  // model has its own switch and the reasoning level it starts with (a new session on it, or switching to it). Unset,
  // a model starts on its provider's recommended level where the provider gives one (ChatGPT does), else on pi's;
  // "pi" keeps pi's even then. Models switched off gather at the bottom of their box, folded.
  let { chat }: { chat?: Chat } = $props();
  const LEVELS: ThinkingLevel[] = ['off', 'minimal', 'low', 'medium', 'high', 'xhigh', 'max'];
  type Level = ThinkingLevel | 'pi' | '';

  let rec = $state<Record<string, Record<string, string>>>({});
  let failed = $state(false);
  $effect(() => {
    api.modelPrefs().then((p) => (app.modelPrefs = p)).catch(() => (failed = true));
    api.recommendedLevels().then((r) => (rec = r)).catch(() => {});
  });
  const prefs = $derived(app.modelPrefs);
  const cur = $derived(chat?.state?.model);

  type Group = { provider: string; models: ModelInfo[] };
  const groups = $derived.by<Group[]>(() => {
    const by = new Map<string, ModelInfo[]>();
    for (const m of chat?.models ?? []) (by.get(m.provider) ?? by.set(m.provider, []).get(m.provider)!).push(m);
    return [...by].map(([provider, models]) => ({ provider, models }));
  });

  // The box of the model in use starts open; the rest folded. Which are open is this dialog's own.
  let open = $state<Record<string, boolean>>({});
  let showOff = $state<Record<string, boolean>>({});
  const isOpen = (p: string) => open[p] ?? p === cur?.provider;

  const key = (m: ModelInfo) => `${m.provider}/${m.id}`;
  const modelOn = (p: ModelPrefs, m: ModelInfo) => !p.hidden.includes(key(m));
  const provOn = (p: ModelPrefs, provider: string) => !p.hiddenProviders.includes(provider);
  const reasoning = (g: Group) => g.models.filter((m) => m.reasoning !== false);
  /** The level a select shows: a stored "pi" reads as the plain default where the provider recommends nothing. */
  const shownLevel = (p: ModelPrefs, m: ModelInfo): Level => {
    const v = p.thinking[key(m)];
    return v === 'pi' && !rec[m.provider]?.[m.id] ? '' : (v ?? '');
  };
  /** The level all of a provider's models share, or null when they differ. */
  const together = (p: ModelPrefs, g: Group): Level | null => {
    const vs = new Set(reasoning(g).map((m) => shownLevel(p, m)));
    return vs.size === 1 ? [...vs][0] : null;
  };

  async function change(provider: string, c: Parameters<typeof api.updateModelPrefs>[1]) {
    try {
      app.modelPrefs = await api.updateModelPrefs(provider, c);
    } catch {
      failed = true;
    }
  }
  const asLevel = (v: string) => (v || null) as ThinkingLevel | 'pi' | null;

  /** The provider's switch: on, off, or half on (some of its models are off). Set here rather than by `checked`, since
      the browser flips the box itself when it is clicked and a value that did not change would not be put back. */
  function provSwitch(node: HTMLInputElement, st: { on: boolean; half: boolean }) {
    const set = (v: { on: boolean; half: boolean }) => { node.checked = v.on; node.indeterminate = v.half; };
    set(st);
    node.addEventListener('change', () => queueMicrotask(() => set(st)));
    return { update: (v: { on: boolean; half: boolean }) => { st = v; set(v); } };
  }
</script>

{#snippet levelSelect(m: ModelInfo, p: ModelPrefs)}
  {@const recommended = rec[m.provider]?.[m.id]}
  <select class="field" aria-label={t('settings.models.levelFor', { model: m.name || m.id })} value={shownLevel(p, m)}
    onchange={(e) => change(m.provider, { model: m.id, level: asLevel(e.currentTarget.value) })}>
    {#if recommended}
      <option value="">{t('settings.models.levelProvider', { level: recommended })}</option>
      <option value="pi">{t('settings.models.levelDefault')}</option>
    {:else}
      <option value="">{t('settings.models.levelDefault')}</option>
    {/if}
    {#each LEVELS as l (l)}<option value={l}>{l}</option>{/each}
  </select>
{/snippet}

{#snippet modelRow(m: ModelInfo, p: ModelPrefs, provOff: boolean)}
  {@const name = m.name || m.id}
  {@const on = modelOn(p, m)}
  <li class:off={!on}>
    <input class="switch" type="checkbox" role="switch" aria-label={t('settings.models.showFor', { model: name })} title={t('settings.models.showFor', { model: name })}
      checked={on} disabled={provOff} onchange={(e) => change(m.provider, { model: m.id, shown: e.currentTarget.checked })} />
    <span class="n">{name}</span>
    {#if m.reasoning !== false}
      {@render levelSelect(m, p)}
    {:else}
      <span class="nolevel">{t('settings.models.noReasoning')}</span>
    {/if}
  </li>
{/snippet}

{#if !chat}
  <p class="note">{t('settings.models.levelsNoChat')}</p>
{:else if failed}
  <p class="note err">{t('settings.models.levelsFailed')}</p>
{:else if prefs}
  <div class="provs">
    {#each groups as g (g.provider)}
      {@const pOn = provOn(prefs, g.provider)}
      {@const onN = pOn ? g.models.filter((m) => modelOn(prefs, m)).length : 0}
      {@const unfolded = isOpen(g.provider)}
      {@const onList = g.models.filter((m) => modelOn(prefs, m))}
      {@const offList = g.models.filter((m) => !modelOn(prefs, m))}
      {@const all = together(prefs, g)}
      {@const name = providerName(g.provider)}
      <section class="prov" class:open={unfolded} class:off={!pOn}>
        <div class="ph">
          <button class="pt" type="button" aria-expanded={unfolded} onclick={() => (open[g.provider] = !unfolded)}>
            <span class="chev" class:open={unfolded} aria-hidden="true"><Icon name="right" size={12} /></span>
            <span class="pn">
              <b data-priv="hosts">{name}</b>
              <small><span class="mono" data-priv="hosts">{g.provider}</span> · {pOn ? t('settings.models.provOn') : t('settings.models.provOff')} <span class="count">{onN}/{g.models.length}</span></small>
            </span>
          </button>
          <input class="switch" type="checkbox" role="switch" aria-label={t('settings.models.showProvider', { provider: name })} title={t('settings.models.showProvider', { provider: name })}
            use:provSwitch={{ on: pOn && onN === g.models.length, half: pOn && onN > 0 && onN < g.models.length }}
            onchange={() => change(g.provider, !pOn ? { providerShown: true } : onN === 0 ? { models: g.models.map((m) => m.id), shown: true } : { providerShown: false })} />
        </div>
        {#if unfolded}
          <div class="pbody">
            {#if reasoning(g).length > 1}
              <div class="pall">
                <span>{t('settings.models.levelAll')}</span>
                <select class="field" aria-label={t('settings.models.levelAllFor', { provider: name })} value={all ?? '~'}
                  onchange={(e) => e.currentTarget.value !== '~' && change(g.provider, { models: reasoning(g).map((m) => m.id), level: asLevel(e.currentTarget.value) })}>
                  {#if all === null}<option value="~" disabled>{t('settings.models.levelMixed')}</option>{/if}
                  <option value="">{t('settings.models.levelDefault')}</option>
                  {#each LEVELS as l (l)}<option value={l}>{l}</option>{/each}
                </select>
              </div>
            {/if}
            <ul>
              {#each onList as m (key(m))}{@render modelRow(m, prefs, !pOn)}{/each}
              {#if offList.length && (showOff[g.provider] || !onList.length)}
                {#each offList as m (key(m))}{@render modelRow(m, prefs, !pOn)}{/each}
              {/if}
            </ul>
            {#if offList.length && onList.length}
              <button class="more" type="button" onclick={() => (showOff[g.provider] = !showOff[g.provider])}>
                {showOff[g.provider] ? t('settings.models.hideOff') : t('settings.models.showOff', { n: offList.length })}
              </button>
            {/if}
          </div>
        {/if}
      </section>
    {/each}
  </div>
{/if}

<style>
  .provs {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 8px;
    margin-top: 8px;
  }
  .prov {
    border: 1px solid var(--rule);
    border-radius: 10px;
    overflow: hidden;
  }
  .ph {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 12px 8px 6px;
  }
  .prov.open .ph {
    border-bottom: 1px solid var(--rule);
    background: var(--paper-sunk);
  }
  .prov.off .pn {
    opacity: 0.55;
  }
  .pt {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 6px;
    text-align: left;
    padding: 2px 4px;
    border-radius: 6px;
    color: inherit;
  }
  .chev {
    flex: none;
    display: inline-grid;
    place-items: center;
    width: 14px;
    color: var(--ink-3);
    transition: transform 0.15s;
  }
  .chev.open {
    transform: rotate(90deg);
  }
  .pn {
    min-width: 0;
    display: grid;
  }
  .pn b {
    font-weight: 620;
    font-size: 14px;
  }
  .pn small {
    font-size: 12px;
    color: var(--ink-3);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .count {
    font-family: var(--font-mono);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
  }
  .pbody {
    padding: 2px 12px 6px 30px;
  }
  .pall {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 0;
    font-size: 13px;
    color: var(--ink-2);
    border-bottom: 1px dashed var(--rule);
  }
  .pall span {
    flex: 1;
    min-width: 0;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    /* a long model name gives way, not the row's width */
    grid-template-columns: minmax(0, 1fr);
  }
  li {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 6px 0;
    border-top: 1px solid var(--rule);
  }
  li:first-child {
    border-top: 0;
  }
  .n {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 14px;
  }
  li.off .n {
    color: var(--ink-3);
  }
  select {
    width: 140px;
    flex: none;
  }
  .nolevel {
    width: 140px;
    flex: none;
    font-size: 12px;
    color: var(--ink-3);
    text-align: center;
  }
  .more {
    margin: 2px 0 2px -6px;
    padding: 3px 6px;
    font-size: 12.5px;
    color: var(--ink-2);
    border-radius: 6px;
  }
  /* half on: some of the provider's models are off */
  .ph :global(input.switch:indeterminate) {
    background: color-mix(in srgb, var(--accent) 45%, var(--rule));
  }
  .ph :global(input.switch:indeterminate::after) {
    translate: 8px 0;
  }
  @media (hover: hover) {
    .pt:hover,
    .more:hover {
      background: var(--rule);
    }
  }
  @media (max-width: 520px) {
    .pbody {
      padding-left: 12px;
    }
    select,
    .nolevel {
      width: 116px;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .chev {
      transition: none;
    }
  }
  .note {
    margin: 6px 0 0;
    color: var(--ink-3);
    font-size: 13px;
  }
  .note.err {
    color: var(--bad);
  }
</style>
