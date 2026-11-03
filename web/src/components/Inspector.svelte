<script lang="ts">
  import { t } from '../lib/i18n.svelte';
  import type { Chat } from '../lib/chat.svelte';
  import { app } from '../lib/app.svelte';
  import { S } from '../lib/strings';
  import Icon from './Icon.svelte';
  import { compactNumber } from '../lib/util';
  import { sessionChanges } from '../lib/changes';
  import DiffTab from './DiffTab.svelte';
  import FilesTab from './FilesTab.svelte';
  import GitTab from './GitTab.svelte';
  import PointTab from './PointTab.svelte';

  let { chat }: { chat: Chat } = $props();

  const ctx = $derived(chat.stats?.contextUsage);
  const pct = $derived(ctx?.percent ?? null);
  const statuses = $derived(Object.entries(chat.statuses));
  const widgets = $derived(Object.entries(chat.widgets));
  const tab = $derived(app.inspectorTab);
  const changed = $derived(sessionChanges(chat.items).length);
  const tabs = $derived([
    { id: 'info', n: 0 },
    { id: 'files', n: 0 },
    { id: 'diff', n: changed },
    { id: 'git', n: 0 },
    { id: 'point', n: 0 },
  ] as const);

  // Arrow keys move between tabs, like any tab list.
  function onTabKey(e: KeyboardEvent) {
    const i = tabs.findIndex((x) => x.id === tab);
    const next = e.key === 'ArrowRight' ? i + 1 : e.key === 'ArrowLeft' ? i - 1 : -1;
    if (next < 0) return;
    e.preventDefault();
    const nt = tabs[(next + tabs.length) % tabs.length];
    app.setInspectorTab(nt.id);
    queueMicrotask(() => (e.currentTarget as HTMLElement).querySelector<HTMLElement>('[aria-selected="true"]')?.focus());
  }
</script>

<aside aria-label={S.inspector}>
  <div class="tabs" role="tablist" tabindex="-1" aria-label={t('inspector.sections')} onkeydown={onTabKey}>
    {#each tabs as tb (tb.id)}
      <button role="tab" type="button" id="itab-{tb.id}" aria-selected={tab === tb.id} aria-controls="ipane" tabindex={tab === tb.id ? 0 : -1} onclick={() => app.setInspectorTab(tb.id)}>
        {t(`inspector.tab.${tb.id}`)}{#if tb.n}<span class="n">{tb.n}</span>{/if}
      </button>
    {/each}
    <span class="grow"></span>
    {#if app.narrow}
      <button class="icon-btn close" type="button" aria-label={t('inspector.close')} onclick={() => (app.inspectorDrawer = false)}><Icon name="x" size={18} /></button>
    {/if}
  </div>

  <div class="pane" id="ipane" role="tabpanel" aria-labelledby="itab-{tab}">
  {#if tab === 'files'}
    <FilesTab {chat} />
  {:else if tab === 'diff'}
    <DiffTab {chat} />
  {:else if tab === 'git'}
    <GitTab {chat} />
  {:else if tab === 'point'}
    <PointTab {chat} />
  {:else}
  <section>
    <h3>{S.context}</h3>
    {#if ctx && pct != null}
      <div class="meter" role="meter" aria-valuenow={Math.round(pct)} aria-valuemin="0" aria-valuemax="100" aria-label={S.context}>
        <i class:warm={pct > 75} style="width:{Math.min(100, pct)}%"></i>
      </div>
      <p class="big">{Math.round(pct)}%</p>
      <p class="sub">{compactNumber(ctx.tokens ?? 0)} / {compactNumber(ctx.contextWindow)} {S.tokens}</p>
    {:else}
      <p class="sub">{ctx ? t('inspector.afterCompact') : t('inspector.noModel')}</p>
    {/if}
  </section>

  {#if chat.stats?.tokens}
    <section>
      <h3>{t('inspector.usage')}</h3>
      <dl>
        <dt>{t('inspector.tokIn')}</dt><dd>{chat.stats.tokens.input.toLocaleString()}</dd>
        <dt>{t('inspector.tokOut')}</dt><dd>{chat.stats.tokens.output.toLocaleString()}</dd>
        <dt>{t('inspector.cacheRead')}</dt><dd>{chat.stats.tokens.cacheRead.toLocaleString()}</dd>
        <dt>{S.cost}</dt><dd>${(chat.stats.cost ?? 0).toFixed((chat.stats.cost ?? 0) < 1 ? 3 : 2)}</dd>
        <dt>{t('inspector.reqRes')}</dt><dd>{chat.stats.userMessages} / {chat.stats.assistantMessages}</dd>
        <dt>{t('inspector.toolCalls')}</dt><dd>{chat.stats.toolCalls}</dd>
      </dl>
    </section>
  {/if}

  {#if statuses.length || widgets.length}
    <section>
      <h3>{S.extensions}</h3>
      <ul class="ext">
        {#each statuses as [k, v] (k)}<li title={k}>{v}</li>{/each}
      </ul>
      {#each widgets as [k, lines] (k)}
        <pre class="mono widget" title={k}>{lines.join('\n')}</pre>
      {/each}
    </section>
  {/if}

  <section>
    <h3>{t('inspector.session')}</h3>
    <p class="sub mono wrap" data-priv="paths">{chat.sessionFile ?? t('inspector.unsaved')}</p>
    {#if chat.runtime.pid}<p class="sub">pid {chat.runtime.pid}</p>{/if}
  </section>
  {/if}
  </div>
</aside>

<style>
  aside {
    height: 100%;
    border-left: 1px solid var(--rule);
    background: var(--paper-sunk);
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  .tabs {
    flex: none;
    overflow-x: auto;
    scrollbar-width: none;
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 8px 10px 0 14px;
    border-bottom: 1px solid var(--rule);
  }
  .tabs [role='tab'] {
    padding: 9px 10px;
    margin-bottom: -1px;
    border-bottom: 2px solid transparent;
    color: var(--ink-2);
    font-size: 13.5px;
    font-weight: 560;
    white-space: nowrap;
  }
  .tabs [role='tab'][aria-selected='true'] {
    color: var(--ink);
    border-bottom-color: var(--heat);
  }
  .n {
    margin-left: 5px;
    padding: 0 6px;
    border-radius: 999px;
    background: var(--accent-wash);
    color: var(--accent);
    font-size: 11.5px;
  }
  .grow {
    flex: 1;
  }
  .close {
    margin-bottom: 4px;
  }
  .pane {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    scrollbar-width: none;
    padding: 16px 16px 24px;
    display: grid;
    align-content: start;
    gap: 22px;
  }
  h3 {
    margin: 0 0 8px;
    font-size: 13px;
    font-weight: 700;
    color: var(--ink-2);
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
  .big {
    margin: 8px 0 0;
    font-size: 30px;
    line-height: 1;
    font-weight: 750;
    letter-spacing: -0.03em;
  }
  .sub {
    margin: 4px 0 0;
    color: var(--ink-3);
    font-size: 13px;
  }
  .wrap {
    overflow-wrap: anywhere;
    font-size: 11.5px;
  }
  dl {
    display: grid;
    grid-template-columns: 1fr auto;
    gap: 5px 12px;
    margin: 0;
    font-size: 13.5px;
  }
  dt {
    color: var(--ink-2);
  }
  dd {
    margin: 0;
    text-align: right;
  }
  .ext {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 4px;
    font-size: 13px;
    color: var(--ink-2);
  }
  .widget {
    margin: 8px 0 0;
    padding: 8px 10px;
    background: var(--code-bg);
    border-radius: 6px;
    font-size: 11.5px;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    max-height: 200px;
    overflow: auto;
  }
  .pane::-webkit-scrollbar {
    display: none;
  }
</style>
