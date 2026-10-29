<script lang="ts">
  import { api, HttpError } from '../lib/api';
  import { app } from '../lib/app.svelte';
  import { showcase } from '../lib/showcase.svelte';
  import type { Chat } from '../lib/chat.svelte';
  import type { SessionSummary } from '../lib/protocol';
  import { contextMenu, type MenuItem } from '../lib/contextmenu.svelte';
  import { S } from '../lib/strings';
  import { baseName, relativeTime, tildePath } from '../lib/util';
  import { tick } from 'svelte';
  import Icon from './Icon.svelte';

  type Entry = { key: string; title: string; when: number; turns: number; session?: SessionSummary; chat?: Chat };
  type Group = { cwd: string; entries: Entry[]; latest: number };

  let query = $state('');
  let searchEl = $state<HTMLInputElement>();
  /** The session whose name is being edited in its row. */
  let renaming = $state<string | null>(null);
  let draft = $state('');
  let expanded = $state<Record<string, boolean>>({});
  const LIMIT = 6;

  const groups = $derived.by<Group[]>(() => {
    const q = query.trim().toLowerCase();
    const byCwd = new Map<string, Entry[]>();
    const add = (cwd: string, e: Entry) => (byCwd.get(cwd) ?? byCwd.set(cwd, []).get(cwd)!).push(e);

    const claimed = new Set<Chat>();
    for (const s of app.sessions) {
      const chat = app.chatFor(s.path);
      if (chat) claimed.add(chat);
      add(s.cwd, {
        key: s.path,
        title: chat?.state?.sessionName || s.name || s.preview || S.untitled,
        when: s.modified, turns: s.turns, session: s, chat,
      });
    }
    // Open sessions pi has not written to disk yet.
    for (const c of app.chats) {
      if (claimed.has(c)) continue;
      add(c.runtime.cwd, { key: c.runtime.id, title: c.title || S.newSession, when: Date.now(), turns: 0, chat: c });
    }

    const out: Group[] = [];
    for (const [cwd, all] of byCwd) {
      const hay = (e: Entry) => `${e.title} ${cwd}`.toLowerCase();
      const entries = (q ? all.filter((e) => hay(e).includes(q)) : all).sort((a, b) => b.when - a.when);
      if (entries.length) out.push({ cwd, entries, latest: entries[0].when });
    }
    return out.sort((a, b) => b.latest - a.latest);
  });

  const rowItems = (g: Group, e: Entry): MenuItem[] => {
    const file = e.chat?.sessionFile ?? e.session?.path;
    if (app.selecting) {
      const on = !!e.session && app.picked.includes(e.session.path);
      return [
        { label: on ? '선택 해제' : '선택', icon: on ? 'x' : 'check', disabled: !e.session || !app.canDelete(e.session), run: () => e.session && app.togglePick(e.session) },
        { label: `${baseName(g.cwd)}의 세션 모두 선택`, icon: 'select', run: () => app.pickMany(deletable(g), true) },
        { label: '모든 세션 선택', icon: 'select', run: () => app.pickMany(allSessions(), true) },
        'sep',
        { label: `선택한 ${app.picked.length}개 삭제…`, icon: 'trash', danger: true, disabled: app.picked.length === 0, run: () => deleteSelected() },
        { label: '선택 모드 끝내기', icon: 'x', run: () => app.stopSelect() },
      ];
    }
    return [
      { label: '열기', icon: 'chat', run: () => open(e) },
      { label: '이름 바꾸기', icon: 'edit', disabled: !e.chat && !e.session, run: () => startRename(e) },
      { label: `${baseName(g.cwd)}에서 ${S.newSession}`, icon: 'plus', run: () => app.openFolder(g.cwd) },
      'sep',
      { label: '세션 트리 보기', icon: 'fork', disabled: !file, run: () => file && (app.treeTarget = { path: file, cwd: g.cwd, title: e.title }) },
      'sep',
      { label: '폴더 경로 복사', icon: 'folder', run: () => app.copy(g.cwd) },
      { label: '세션 파일 경로 복사', icon: 'link', disabled: !file, run: () => app.copy(file ?? '') },
      'sep',
      ...(e.chat ? [{ label: S.closeSession, icon: 'x', run: () => app.closeChat(e.chat!) } as MenuItem] : []),
      ...(e.session && !e.chat ? [{ label: S.deleteSession, icon: 'trash', danger: true, run: () => remove(e) } as MenuItem] : []),
      ...(e.session && !e.chat ? [{ label: '여러 개 선택해서 삭제…', icon: 'select', danger: true, run: () => app.startSelect(e.session) } as MenuItem] : []),
    ];
  };
  const groupItems = (g: Group): MenuItem[] => app.selecting
    ? [
        { label: `${baseName(g.cwd)}의 세션 모두 선택`, icon: 'select', run: () => app.pickMany(deletable(g), true) },
        { label: `${baseName(g.cwd)}의 선택 해제`, icon: 'x', run: () => app.pickMany(deletable(g), false) },
      ]
    : [
        { label: `${baseName(g.cwd)}에서 ${S.newSession}`, icon: 'plus', run: () => app.openFolder(g.cwd) },
        { label: '폴더 경로 복사', icon: 'folder', run: () => app.copy(g.cwd) },
        'sep',
        { label: '이 폴더의 세션 여러 개 삭제…', icon: 'select', danger: true, disabled: deletable(g).length === 0, run: () => { app.startSelect(); app.pickMany(deletable(g), false); } },
      ];

  const deletable = (g: Group) => g.entries.flatMap((e) => (e.session && app.canDelete(e.session) ? [e.session] : []));
  const allSessions = () => groups.flatMap(deletable);
  const groupState = (g: Group): 'all' | 'some' | 'none' => {
    const list = deletable(g);
    const n = list.filter((s) => app.picked.includes(s.path)).length;
    return n === 0 ? 'none' : n === list.length ? 'all' : 'some';
  };

  function open(e: Entry) {
    if (app.selecting) {
      if (e.session) app.togglePick(e.session);
      return;
    }
    if (e.chat) app.select(e.chat.runtime.id);
    else if (e.session) void app.openSession(e.session);
  }

  function remove(e: Entry) {
    if (e.session) void app.deleteSessions([e.session]);
  }

  function deleteSelected() {
    const list = app.sessions.filter((s) => app.picked.includes(s.path));
    void app.deleteSessions(list);
  }

  const focusSelect = (node: HTMLInputElement) => { node.focus(); node.select(); };

  function startRename(e: Entry) {
    draft = e.chat?.state?.sessionName || e.session?.name || (e.title === S.untitled || e.title === S.newSession ? '' : e.title);
    renaming = e.key;
  }

  async function commitRename(e: Entry) {
    if (renaming !== e.key) return;
    renaming = null;
    const name = draft.trim();
    const was = e.chat?.state?.sessionName || e.session?.name || '';
    if (name === was) return;
    if (e.chat) {
      if (name) await e.chat.rename(name); // an open session is renamed by its own pi
    } else if (e.session) {
      try {
        await api.renameSession(e.session.path, name);
        await app.refreshSessions();
      } catch (err) {
        app.flash(err instanceof HttpError && err.status === 409 ? '지금 열려 있어서 바꿀 수 없습니다' : '이름을 바꾸지 못했습니다');
      }
    }
  }

  async function searchFromRail() {
    app.toggleSidebarFold();
    await tick();
    searchEl?.focus();
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape' && app.selecting && !app.dialog && !document.querySelector('dialog[open]')) app.stopSelect();
  }
</script>

<svelte:window onkeydown={onKey} />

<aside class:rail={app.rail}>
{#if app.rail}
  <div class="strip">
    <img class="mark" src="/logo.svg" alt="pi-oven" width="28" height="28" />
    <button class="icon-btn" type="button" aria-label="사이드바 펼치기" title="사이드바 펼치기" onclick={() => app.toggleSidebarFold()}><Icon name="sidebar" size={20} /></button>
    <button class="icon-btn open" type="button" aria-label={S.openFolder} title={S.openFolder} onclick={() => (app.folderPicker = true)}><Icon name="folder" size={20} /></button>
    <button class="icon-btn" type="button" aria-label={S.searchSessions} title={S.searchSessions} onclick={searchFromRail}><Icon name="search" size={20} /></button>
    <div class="chats" role="group" aria-label="열린 세션">
      {#each app.chats as c (c.runtime.id)}
        {@const t = c.state?.sessionName || c.title || baseName(c.runtime.cwd)}
        <button class="chat" class:cur={c.runtime.id === app.currentId} type="button" title={showcase.tip('titles', `${t} (${baseName(c.runtime.cwd)})`)} aria-label={showcase.active('titles') ? '세션' : t}
          aria-current={c.runtime.id === app.currentId ? 'true' : undefined} onclick={() => app.select(c.runtime.id)}>
          <span class="ch" data-priv="paths" aria-hidden="true">{Array.from(baseName(c.runtime.cwd))[0]?.toUpperCase() ?? '?'}</span>
          <span class="state" class:run={c.running} class:unseen={c.unseen} class:bg={!c.running && c.bg.running > 0} aria-hidden="true"></span>
        </button>
      {/each}
    </div>
    <button class="icon-btn" class:showing={showcase.on} type="button" aria-label="설정" title={showcase.on ? '설정 (쇼케이스 모드 켜짐)' : '설정'} onclick={() => (app.settingsOpen = true)}><Icon name={showcase.on ? 'eyeOff' : 'gear'} size={20} /></button>
  </div>
{:else}
  <div class="brand">
    <img class="mark" src="/logo.svg" alt="" width="28" height="28" />
    <span class="word">pi-oven</span>
    {#if app.wide}
      <button class="icon-btn fold" type="button" aria-label="사이드바 접기" title="사이드바 접기" onclick={() => app.toggleSidebarFold()}><Icon name="sidebar" size={18} /></button>
    {/if}
    <button class="icon-btn close" type="button" aria-label="목록 닫기" onclick={() => (app.sidebarOpen = false)}><Icon name="x" /></button>
  </div>

  <div class="actions">
    <button class="btn primary wide" type="button" onclick={() => (app.folderPicker = true)}>
      <Icon name="folder" size={16} />{S.openFolder}
    </button>
    <input class="field" type="search" bind:this={searchEl} placeholder={S.searchSessions} aria-label={S.searchSessions} bind:value={query} />
  </div>

  <nav aria-label="세션">
    {#each groups as g (g.cwd)}
      {@const shown = expanded[g.cwd] || query ? g.entries : g.entries.slice(0, LIMIT)}
      <section>
        <div class="gh" use:contextMenu={() => groupItems(g)}>
          {#if app.selecting}
            {@const st = groupState(g)}
            <button class="box" class:on={st !== 'none'} type="button" role="checkbox" aria-checked={st === 'all' ? 'true' : st === 'some' ? 'mixed' : 'false'}
              aria-label="{baseName(g.cwd)}의 세션 모두 선택" disabled={deletable(g).length === 0}
              onclick={() => app.pickMany(deletable(g), st !== 'all')}>
              {#if st === 'all'}<Icon name="check" size={12} />{:else if st === 'some'}<span class="dash"></span>{/if}
            </button>
          {/if}
          <div class="gt" title={showcase.tip('paths', g.cwd)}>
            <span class="gname" data-priv="paths">{baseName(g.cwd)}</span>
            <span class="gpath mono" data-priv="paths">{tildePath(g.cwd, app.home)}</span>
          </div>
          <button class="icon-btn sm" class:hide={app.selecting} type="button" title="{S.newSession} ({baseName(g.cwd)})" aria-label="{baseName(g.cwd)}에서 {S.newSession}" onclick={() => app.openFolder(g.cwd)}>
            <Icon name="plus" size={16} />
          </button>
        </div>
        <ul>
          {#each shown as e (e.key)}
            {@const checked = !!e.session && app.picked.includes(e.session.path)}
            {@const locked = app.selecting && (!e.session || !app.canDelete(e.session))}
            <li class:active={!app.selecting && e.chat && e.chat.runtime.id === app.currentId} class:checked>
              {#if renaming === e.key}
                <div class="row editing">
                  <span class="state" class:open={!!e.chat} aria-hidden="true"></span>
                  <input class="rename" aria-label="세션 이름" placeholder="세션 이름" bind:value={draft} use:focusSelect
                    onkeydown={(ev) => { if (ev.isComposing) return; if (ev.key === 'Enter') { ev.preventDefault(); void commitRename(e); } else if (ev.key === 'Escape') { ev.preventDefault(); renaming = null; } }}
                    onblur={() => void commitRename(e)} />
                </div>
              {:else}
              <button class="row" class:selecting={app.selecting} class:locked type="button" onclick={() => open(e)} use:contextMenu={() => rowItems(g, e)}
                role={app.selecting ? 'checkbox' : undefined} aria-checked={app.selecting ? checked : undefined} aria-disabled={locked || undefined}
                title={locked ? '열려 있는 세션은 먼저 닫아야 삭제할 수 있어요' : undefined}
                aria-current={!app.selecting && e.chat && e.chat.runtime.id === app.currentId ? 'true' : undefined}>
                {#if app.selecting}
                  <span class="box" class:on={checked} aria-hidden="true">{#if checked}<Icon name="check" size={12} />{/if}</span>
                {:else}
                <span class="state" class:run={e.chat?.running} class:open={!!e.chat} class:unseen={e.chat?.unseen} class:bg={!e.chat?.running && (e.chat?.bg.running ?? 0) > 0} role="img" aria-label={e.chat?.running ? S.running : e.chat ? '열림' : ''}></span>
                {/if}
                <span class="t" data-priv="titles">{e.title}</span>
                <span class="m">{relativeTime(e.when)}</span>
              </button>
              {/if}
              {#if e.session && !e.chat && !app.selecting && renaming !== e.key}
                <button class="del icon-btn sm" type="button" aria-label={S.deleteSession} title={S.deleteSession} onclick={() => remove(e)}>
                  <Icon name="trash" size={15} />
                </button>
              {/if}
            </li>
          {/each}
        </ul>
        {#if !query && g.entries.length > LIMIT}
          <button class="more" type="button" onclick={() => (expanded[g.cwd] = !expanded[g.cwd])}>
            {expanded[g.cwd] ? S.showLess : `${S.showMore} (${g.entries.length - LIMIT})`}
          </button>
        {/if}
      </section>
    {:else}
      <p class="empty">{query ? S.noMatch : S.noSessions}</p>
    {/each}
  </nav>

  {#if app.selecting}
    <div class="selbar" role="toolbar" aria-label="선택한 세션 삭제">
      <span class="count" aria-live="polite">{app.picked.length}개 선택</span>
      <span class="grow"></span>
      <button class="btn quiet" type="button" onclick={() => app.pickMany(allSessions(), app.picked.length < allSessions().length)}>
        {app.picked.length < allSessions().length ? '전체 선택' : '선택 해제'}
      </button>
      <button class="btn danger" type="button" disabled={app.picked.length === 0} onclick={deleteSelected}>
        <Icon name="trash" size={15} />삭제
      </button>
      <button class="icon-btn" type="button" aria-label="선택 모드 끝내기" title="선택 모드 끝내기 (Esc)" onclick={() => app.stopSelect()}><Icon name="x" size={18} /></button>
    </div>
  {/if}

  <footer>
    <button class="btn quiet set" type="button" onclick={() => (app.settingsOpen = true)}>
      <Icon name="gear" size={16} />설정
    </button>
    {#if showcase.on}
      <button class="showtag" type="button" title="쇼케이스 모드가 켜져 있습니다. 누르면 설정이 열립니다." onclick={() => (app.settingsOpen = true)}><Icon name="eyeOff" size={13} />쇼케이스</button>
    {/if}
  </footer>
{/if}
</aside>

<style>
  aside {
    height: 100%;
    display: flex;
    flex-direction: column;
    background: var(--paper-sunk);
    border-right: 1px solid var(--rule);
    min-height: 0;
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 18px 18px 10px;
    letter-spacing: -0.04em;
  }
  .mark {
    display: block;
  }
  .close {
    display: none;
    margin-left: auto;
  }
  .fold {
    margin-left: auto;
  }
  /* The list keeps its own width while the column around it animates, so it is clipped rather than
     re-wrapped: no text squeezing into "…" on the way. */
  @media (min-width: 901px) {
    aside {
      width: var(--sidebar-full, 292px);
    }
    aside.rail {
      width: 56px;
    }
  }
  @media (max-width: 900px) {
    .fold {
      display: none;
    }
  }
  .strip {
    height: 100%;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    padding: 16px 0 12px;
    min-height: 0;
  }
  .strip .mark {
    margin-bottom: 8px;
  }
  .strip .icon-btn {
    width: 40px;
    height: 40px;
  }
  .strip .open {
    color: var(--heat);
  }
  .chats {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    scrollbar-width: none;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    width: 100%;
    padding: 10px 0;
    margin-top: 6px;
    border-top: 1px solid var(--rule);
  }
  .chat {
    position: relative;
    width: 36px;
    height: 36px;
    flex: none;
    border-radius: 10px;
    background: var(--raised);
    border: 1px solid var(--rule);
    display: grid;
    place-items: center;
    font-weight: 700;
    font-size: 14px;
    color: var(--ink-2);
  }
  .chat.cur {
    border-color: var(--accent);
    color: var(--ink);
    box-shadow: inset 0 0 0 1px var(--accent);
  }
  .chat .state {
    position: absolute;
    right: -3px;
    top: -3px;
    outline: 2px solid var(--paper-sunk);
  }
  @media (max-width: 900px) {
    .close {
      display: inline-grid;
    }
  }
  .word {
    font-size: 20px;
    font-weight: 700;
  }
  .actions {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 8px;
    padding: 6px 14px 12px;
  }
  .wide {
    justify-content: center;
    height: 36px;
  }
  nav {
    flex: 1;
    overflow-x: hidden;
    overflow-y: auto;
    scrollbar-width: none;
    padding: 0 8px 12px;
  }
  section {
    margin-top: 14px;
  }
  .gh {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 0 6px 2px 10px;
  }
  .gt {
    flex: 1;
    min-width: 0;
    display: grid;
    line-height: 1.3;
  }
  .gname {
    font-weight: 680;
    font-size: 13.5px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .gpath {
    font-size: 11px;
    color: var(--ink-3);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sm {
    width: 28px;
    height: 28px;
  }
  ul {
    list-style: none;
    margin: 2px 0 0;
    padding: 0;
  }
  li {
    position: relative;
    border-radius: 8px;
  }
  li.active {
    background: var(--raised);
    box-shadow: inset 3px 0 0 var(--accent);
  }
  .row {
    -webkit-touch-callout: none;
    user-select: none;
    width: 100%;
    display: grid;
    grid-template-columns: 10px minmax(0, 1fr) auto;
    align-items: center;
    gap: 10px;
    padding: 7px 10px;
    text-align: left;
    border-radius: 8px;
  }
  .row.editing {
    cursor: text;
  }
  .rename {
    min-width: 0;
    height: 26px;
    padding: 0 8px;
    border-radius: 6px;
    border: 1px solid var(--accent);
    background: var(--raised);
    color: var(--ink);
    font: inherit;
    font-size: 14px;
  }
  .rename:focus {
    outline: none;
  }
  .row.editing .state {
    justify-self: start;
  }
  @media (hover: hover) {
    li:not(.active) .row:hover {
      background: color-mix(in srgb, var(--rule) 45%, transparent);
    }
  }

  .t {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 14px;
  }
  li.active .t {
    font-weight: 620;
  }
  .m {
    font-size: 11.5px;
    color: var(--ink-3);
  }
  .state {
    width: 7px;
    height: 7px;
    border-radius: 2px;
  }
  .state.open {
    background: var(--ink-3);
  }
  .state.bg {
    background: transparent;
    box-shadow: inset 0 0 0 1.5px var(--heat);
    animation: ember 1.6s ease-in-out infinite;
  }
  .state.unseen {
    background: var(--accent);
  }
  .state.run {
    background: var(--heat);
    animation: ember 1.1s ease-in-out infinite;
  }
  @keyframes ember {
    50% {
      box-shadow: 0 0 0 4px color-mix(in srgb, var(--heat) 28%, transparent);
    }
  }
  .del {
    position: absolute;
    right: 2px;
    top: 50%;
    translate: 0 -50%;
    display: none;
    background: var(--paper-sunk);
  }
  @media (hover: hover) {
    li:hover .del,
    .del:focus-visible {
      display: inline-grid;
    }
  }

  @media (hover: hover) {
    li:hover:has(.del) .m {
      visibility: hidden;
    }
  }

  .box {
    flex: none;
    width: 18px;
    height: 18px;
    border-radius: 5px;
    border: 1.5px solid var(--ink-3);
    display: grid;
    place-items: center;
    color: var(--accent-ink);
    background: transparent;
  }
  .box.on {
    background: var(--accent);
    border-color: var(--accent);
  }
  .box:disabled {
    opacity: 0.35;
  }
  .dash {
    width: 8px;
    height: 2px;
    border-radius: 1px;
    background: var(--accent-ink);
  }
  .gh .box {
    margin-right: 8px;
  }
  .icon-btn.hide {
    visibility: hidden;
  }
  .row.selecting {
    grid-template-columns: 18px minmax(0, 1fr) auto;
  }
  .row.locked {
    opacity: 0.45;
    cursor: not-allowed;
  }
  li.checked {
    background: var(--accent-wash);
  }
  .selbar {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 8px 10px;
    border-top: 1px solid var(--rule);
    background: var(--paper-sunk);
  }
  .count {
    font-weight: 650;
    font-size: 13.5px;
    padding-left: 4px;
  }
  .grow {
    flex: 1;
  }
  .more {
    margin: 2px 0 0 10px;
    padding: 2px 8px;
    font-size: 12.5px;
    color: var(--ink-2);
    border-radius: 6px;
  }
  @media (hover: hover) {
    .more:hover {
      background: var(--rule);
    }
  }

  .empty {
    color: var(--ink-3);
    padding: 20px 12px;
    font-size: 13.5px;
  }
  .set {
    width: 100%;
    justify-content: flex-start;
  }
  .showtag {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    margin-left: auto;
    padding: 3px 10px;
    border-radius: 999px;
    border: 1px solid color-mix(in srgb, var(--accent) 50%, var(--rule));
    background: var(--accent-wash);
    color: var(--accent);
    font-size: 12px;
    font-weight: 600;
    white-space: nowrap;
    flex: none;
  }
  footer {
    gap: 4px;
    padding: 8px 12px;
    border-top: 1px solid var(--rule);
    display: flex;
  }
  nav::-webkit-scrollbar {
    display: none;
  }
</style>
