<script lang="ts">
  import { app } from '../lib/app.svelte';
  import { showcase } from '../lib/showcase.svelte';
  import type { Chat } from '../lib/chat.svelte';
  import { dismiss, autofocus } from '../lib/actions';
  import { S } from '../lib/strings';
  import { tildePath } from '../lib/util';
  import Icon from './Icon.svelte';

  let { chat }: { chat: Chat } = $props();

  let editing = $state(false);
  let draft = $state('');
  let menu = $state<'more' | null>(null);

  const title = $derived(chat.title || S.untitled);

  function startRename() {
    draft = chat.state?.sessionName ?? chat.title;
    editing = true;
  }
  function commitRename() {
    editing = false;
    const name = draft.trim();
    if (name && name !== chat.state?.sessionName) void chat.rename(name);
  }
</script>

<header>
  <button class="icon-btn menu" type="button" aria-label="세션 목록" onclick={() => (app.sidebarOpen = true)}>
    <Icon name="menu" />
  </button>

  <div class="id">
    {#if editing}
      <input class="field name" bind:value={draft} use:autofocus onblur={commitRename}
        onkeydown={(e) => { if (e.key === 'Enter' && !e.isComposing) commitRename(); if (e.key === 'Escape') editing = false; }} />
    {:else}
      <button class="title" type="button" onclick={startRename} title={S.rename} data-priv="titles">{title}</button>
    {/if}
    <span class="cwd mono" data-priv="paths">{tildePath(chat.runtime.cwd, app.home)}</span>
  </div>

  <div class="tools">
    <div class="pop" use:dismiss={() => { if (menu === 'more') menu = null; }}>
      <button class="icon-btn" type="button" aria-label="더 보기" aria-haspopup="menu" aria-expanded={menu === 'more'} onclick={() => (menu = menu === 'more' ? null : 'more')}>
        <Icon name="more" />
      </button>
      {#if menu === 'more'}
        <div class="sheet narrow" role="menu">
          <button role="menuitem" type="button" onclick={() => ((menu = null), startRename())}>{S.rename}</button>
          <button role="menuitem" type="button" disabled={chat.running || chat.compacting} onclick={() => ((menu = null), chat.compact())}>{S.compact}</button>
          <button role="menuitem" type="button" disabled={chat.running} onclick={() => ((menu = null), chat.newSession())}>{S.newSession}</button>
          <hr />
          <button role="menuitem" type="button" disabled={!chat.sessionFile} onclick={() => ((menu = null), chat.sessionFile && (app.treeTarget = { path: chat.sessionFile, cwd: chat.runtime.cwd, title }))}>세션 트리</button>
          <button role="menuitem" type="button" onclick={() => ((menu = null), app.openInspector('diff'))}>변경 파일</button>
          <hr />
          <button role="menuitem" type="button" onclick={() => ((menu = null), app.closeChat(chat))}>{S.closeSession}</button>
        </div>
      {/if}
    </div>

    <button class="icon-btn" type="button" aria-label={S.inspector} aria-pressed={app.inspectorOpen} onclick={() => app.toggleInspector()}>
      <Icon name="panel" />
    </button>
  </div>
</header>

{#if chat.link !== 'open' || chat.compacting || chat.retry}
  <div class="strip" class:bad={chat.link === 'exited'} role="status">
    {#if chat.link === 'exited'}
      {S.exited}
      <button class="btn" type="button" onclick={() => app.closeChat(chat)}>{S.closeSession}</button>
    {:else if chat.link === 'reconnecting'}{S.reconnecting}
    {:else if chat.link === 'connecting'}{S.connecting}
    {:else if chat.compacting}{S.compacting}
    {:else}{S.retrying} {chat.retry}{/if}
  </div>
{/if}

<style>
  header {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 20px 10px 24px;
    border-bottom: 1px solid var(--rule);
    min-height: 62px;
  }
  .menu {
    display: none;
  }
  .id {
    flex: 1;
    min-width: 0;
    display: grid;
    line-height: 1.3;
  }
  .title {
    justify-self: start;
    max-width: 100%;
    font-weight: 680;
    font-size: 16px;
    letter-spacing: -0.01em;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    text-align: left;
    border-radius: 4px;
  }
  @media (hover: hover) {
    .title:hover {
      text-decoration: underline;
      text-decoration-color: var(--rule);
      text-underline-offset: 4px;
    }
  }

  .name {
    font-weight: 680;
    font-size: 16px;
    width: min(100%, 420px);
  }
  .cwd {
    color: var(--ink-3);
    font-size: 12px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .tools {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .pop {
    position: relative;
  }
  .sheet {
    position: absolute;
    right: 0;
    top: calc(100% + 6px);
    width: 320px;
    max-height: min(420px, 70vh);
    overflow: auto;
    z-index: 20;
    padding: 6px;
    background: var(--raised);
    border: 1px solid var(--rule);
    border-radius: 10px;
    box-shadow: var(--shadow);
    display: grid;
    gap: 4px;
  }
  .sheet.narrow {
    width: 200px;
  }
  .sheet button {
    width: 100%;
    text-align: left;
    padding: 7px 10px;
    border-radius: 6px;
    display: flex;
    justify-content: space-between;
    gap: 12px;
    align-items: baseline;
  }
  @media (hover: hover) {
    .sheet button:hover:not(:disabled),
    .sheet button:disabled {
      color: var(--ink-3);
      cursor: not-allowed;
    }
  }

  hr {
    border: 0;
    border-top: 1px solid var(--rule);
    margin: 2px 0;
    width: 100%;
  }
  .strip {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 12px;
    padding: 6px 16px;
    font-size: 13.5px;
    background: var(--heat-wash);
    color: var(--ink);
  }
  .strip.bad {
    background: color-mix(in srgb, var(--bad) 18%, var(--paper));
  }
  @media (max-width: 900px) {
    .menu {
      display: inline-grid;
    }
    header {
      padding-inline: 12px;
    }
  }
</style>
