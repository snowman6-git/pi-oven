<script lang="ts">
  import { S } from '../lib/strings';
  import { t } from '../lib/i18n.svelte';
  import { app } from '../lib/app.svelte';
  import { showcase } from '../lib/showcase.svelte';
  import type { Chat } from '../lib/chat.svelte';
  import { contextMenu, type MenuItem } from '../lib/contextmenu.svelte';
  import FileIcon from './FileIcon.svelte';
  import Icon from './Icon.svelte';

  let { chat, splitOn = false, canSplit = false }: { chat: Chat; splitOn?: boolean; canSplit?: boolean } = $props();
  const editors = $derived(chat.editors);
  /** The tab that is picked: in a split the conversation is always there, so there is always a file. */
  const picked = $derived(splitOn ? (editors.active ?? editors.last) : editors.active);
  let strip = $state<HTMLElement>();

  // The tab in view stays in view when there are more than fit.
  $effect(() => {
    editors.active;
    queueMicrotask(() => strip?.querySelector('[aria-selected="true"]')?.scrollIntoView({ inline: 'nearest', block: 'nearest' }));
  });

  const items = (path: string): MenuItem[] => [
    { label: S.close, icon: 'x', run: () => void editors.close(path) },
    { label: t('tabs.closeOthers'), icon: 'x', disabled: editors.docs.length < 2, run: () => void editors.closeOthers(path) },
    { label: t('tabs.closeAll'), icon: 'x', run: async () => { for (const d of [...editors.docs]) if (!(await editors.close(d.path))) break; } },
    'sep',
    { label: t('files.copyPath'), icon: 'link', run: () => app.copy(path) },
  ];

  function onKey(e: KeyboardEvent) {
    const order = [...(splitOn ? [] : [null]), ...editors.docs.map((d) => d.path)];
    const i = order.indexOf(picked);
    const next = e.key === 'ArrowRight' ? i + 1 : e.key === 'ArrowLeft' ? i - 1 : -1;
    if (next < 0 || next >= order.length) return;
    e.preventDefault();
    editors.activate(order[next]);
    queueMicrotask(() => strip?.querySelector<HTMLElement>('[aria-selected="true"]')?.focus());
  }
</script>

<div class="tabs" role="tablist" aria-label={t('tabs.label')} tabindex="-1" bind:this={strip} onkeydown={onKey}>
  {#if !splitOn}
    <button class="tab chat" role="tab" type="button" aria-selected={editors.active === null} tabindex={editors.active === null ? 0 : -1} onclick={() => editors.activate(null)}>
      {t('tabs.chat')}{#if chat.unreadReply}<i class="dot" role="img" aria-label={t('tabs.newReply')}></i>{/if}
    </button>
  {/if}
  {#each editors.docs as d (d.path)}
    {@const on = picked === d.path}
    <span class="file" class:on class:dirty={d.dirty} use:contextMenu={() => items(d.path)}>
      <button class="tab" role="tab" type="button" aria-selected={on} tabindex={on ? 0 : -1} title={showcase.tip('files', d.path)} onclick={() => editors.activate(d.path)}
        onauxclick={(e) => { if (e.button === 1) { e.preventDefault(); void editors.close(d.path); } }}>
        <FileIcon name={d.name} size={14} />
        <span class="nm" data-priv="files">{d.name}</span>
        {#if d.dirty}<i class="dot" role="img" aria-label={t('tabs.unsaved')}></i>{/if}
      </button>
      <button class="x" type="button" aria-label={t('tabs.closeName', { name: d.name })} onclick={() => editors.close(d.path)}>×</button>
    </span>
  {/each}
  {#if canSplit || splitOn}
    <button class="split" type="button" aria-pressed={splitOn} title={splitOn ? t('tabs.asTabs') : t('tabs.sideBySide')} aria-label={splitOn ? t('tabs.asTabs') : t('tabs.sideBySide')} onclick={() => editors.setSplit(!editors.split)}>
      <Icon name="split" size={16} />
    </button>
  {/if}
</div>

<style>
  .tabs {
    flex: none;
    display: flex;
    align-items: stretch;
    gap: 2px;
    padding: 6px 12px 0;
    border-bottom: 1px solid var(--rule);
    background: var(--paper-sunk);
    overflow-x: auto;
    scrollbar-width: none;
  }
  .tabs::-webkit-scrollbar {
    display: none;
  }
  .tab {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 34px;
    padding: 0 10px;
    border-radius: 8px 8px 0 0;
    border-bottom: 2px solid transparent;
    color: var(--ink-2);
    font-size: 13px;
    font-weight: 560;
    white-space: nowrap;
  }
  .tab[aria-selected='true'] {
    color: var(--ink);
    border-bottom-color: var(--accent);
  }
  .file {
    display: inline-flex;
    align-items: center;
    flex: none;
    max-width: 220px;
    border-radius: 8px 8px 0 0;
  }
  .file.on {
    background: var(--paper);
  }
  .file .tab {
    min-width: 0;
    padding-right: 4px;
  }
  .nm {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .dot {
    flex: none;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--accent);
  }
  .split {
    flex: none;
    margin: 0 0 4px auto;
    align-self: center;
    width: 30px;
    height: 28px;
    display: inline-grid;
    place-items: center;
    border-radius: 6px;
    color: var(--ink-3);
  }
  .split[aria-pressed='true'] {
    color: var(--accent);
    background: var(--accent-wash);
  }
  @media (hover: hover) {
    .split:not([aria-pressed='true']):hover {
      background: var(--rule);
      color: var(--ink);
    }
  }
  .x {
    flex: none;
    width: 22px;
    height: 22px;
    margin-right: 4px;
    border-radius: 6px;
    color: var(--ink-3);
    font-size: 15px;
    line-height: 1;
  }
  @media (hover: hover) {
    .x:hover {
      background: var(--rule);
      color: var(--ink);
    }
  }
  @media (max-width: 720px) {
    .tabs {
      padding-inline: 8px;
    }
  }
</style>
