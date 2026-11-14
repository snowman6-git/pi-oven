<script lang="ts">
  import { api, HttpError } from '../lib/api';
  import { app } from '../lib/app.svelte';
  import { showcase } from '../lib/showcase.svelte';
  import type { Chat } from '../lib/chat.svelte';
  import type { SessionSummary } from '../lib/protocol';
  import { contextMenu, menu, type MenuItem } from '../lib/contextmenu.svelte';
  import { i18n, LANGS, setLang, t } from '../lib/i18n.svelte';
  import { S } from '../lib/strings';
  import { baseName, relativeTime, tildePath } from '../lib/util';
  import { tick } from 'svelte';
  import Icon from './Icon.svelte';
  import UpdateCard from './UpdateCard.svelte';

  type Entry = { key: string; title: string; when: number; turns: number; session?: SessionSummary; chat?: Chat };
  type Group = { cwd: string; entries: Entry[]; latest: number };

  let query = $state('');
  let searchEl = $state<HTMLInputElement>();
  /** The session whose name is being edited in its row. */
  let renaming = $state<string | null>(null);
  let draft = $state('');
  let expanded = $state<Record<string, boolean>>({});
  const LIMIT = 6;

  // Folded folders show only their open sessions; the rest wait until the folder is unfolded. Folded is the default,
  // and the unfolded ones are remembered in this browser.
  const OPEN_KEY = 'pi-oven.folders-open';
  let unfolded = $state<string[]>((() => {
    try { const v = JSON.parse(localStorage.getItem(OPEN_KEY) ?? '[]'); return Array.isArray(v) ? v.filter((x) => typeof x === 'string') : []; } catch { return []; }
  })());
  const isOpen = (cwd: string) => unfolded.includes(cwd);
  function toggleFold(cwd: string) {
    unfolded = isOpen(cwd) ? unfolded.filter((c) => c !== cwd) : [...unfolded, cwd];
    try { localStorage.setItem(OPEN_KEY, JSON.stringify(unfolded)); } catch { /* private mode */ }
  }

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
        { label: on ? t('sidebar.unselect') : t('sidebar.select'), icon: on ? 'x' : 'check', disabled: !e.session || !app.canDelete(e.session), run: () => e.session && app.togglePick(e.session) },
        { label: t('sidebar.selectFolder', { folder: shortName(g.cwd) }), icon: 'select', run: () => app.pickMany(deletable(g), true) },
        { label: t('sidebar.selectAll'), icon: 'select', run: () => app.pickMany(allSessions(), true) },
        'sep',
        { label: t('sidebar.deletePicked', { n: app.picked.length }), icon: 'trash', danger: true, disabled: app.picked.length === 0, run: () => deleteSelected() },
        { label: t('sidebar.endSelect'), icon: 'x', run: () => app.stopSelect() },
      ];
    }
    return [
      { label: t('sidebar.open'), icon: 'chat', run: () => open(e) },
      { label: S.rename, icon: 'edit', disabled: !e.chat && !e.session, run: () => startRename(e) },
      { label: t('sidebar.newIn', { folder: shortName(g.cwd) }), icon: 'plus', run: () => app.openFolder(g.cwd) },
      'sep',
      { label: t('sidebar.tree'), icon: 'fork', disabled: !file, run: () => file && (app.treeTarget = { path: file, cwd: g.cwd, title: e.title }) },
      { label: t('sidebar.move'), icon: 'folderOpen', disabled: !e.session, run: () => e.session && app.startMove(e.session) },
      'sep',
      { label: t('sidebar.copyFolder'), icon: 'folder', run: () => app.copy(g.cwd) },
      { label: t('sidebar.copyFile'), icon: 'link', disabled: !file, run: () => app.copy(file ?? '') },
      'sep',
      ...(e.chat ? [{ label: S.closeSession, icon: 'x', run: () => app.closeChat(e.chat!) } as MenuItem] : []),
      ...(e.session ? [{ label: S.deleteSession, icon: 'trash', danger: true, run: () => remove(e) } as MenuItem] : []),
      ...(e.session ? [{ label: t('sidebar.selectToDelete'), icon: 'select', danger: true, run: () => app.startSelect(e.session) } as MenuItem] : []),
    ];
  };
  const groupItems = (g: Group): MenuItem[] => app.selecting
    ? [
        { label: t('sidebar.selectFolder', { folder: shortName(g.cwd) }), icon: 'select', run: () => app.pickMany(deletable(g), true) },
        { label: t('sidebar.unselectFolder', { folder: shortName(g.cwd) }), icon: 'x', run: () => app.pickMany(deletable(g), false) },
      ]
    : [
        { label: t('sidebar.newIn', { folder: shortName(g.cwd) }), icon: 'plus', run: () => app.openFolder(g.cwd) },
        { label: isOpen(g.cwd) ? t('sidebar.fold') : t('sidebar.unfold'), icon: isOpen(g.cwd) ? 'up' : 'down', run: () => toggleFold(g.cwd) },
        { label: t('sidebar.copyFolder'), icon: 'folder', run: () => app.copy(g.cwd) },
        'sep',
        { label: t('sidebar.deleteInFolder'), icon: 'select', danger: true, disabled: deletable(g).length === 0, run: () => { app.startSelect(); app.pickMany(deletable(g), false); } },
      ];

  /** A folder name for a menu label: a long one keeps its start and end, so the rest of the label still shows. */
  const shortName = (cwd: string) => {
    const n = baseName(cwd);
    return n.length > 16 ? `${n.slice(0, 10)}…${n.slice(-5)}` : n;
  };
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
        app.flash(err instanceof HttpError && err.status === 409 ? t('sidebar.renameOpen') : t('sidebar.renameFailed'));
      }
    }
  }

  async function searchFromRail() {
    app.toggleSidebarFold();
    await tick();
    searchEl?.focus();
  }

  const upd = $derived(app.updates);
  const updSummary = $derived.by(() => {
    const o = upd.offer;
    if (!o) return '';
    const parts = [];
    if (o.pi) parts.push(`pi ${o.pi[0]} → ${o.pi[1]}`);
    if (o.extensions.length) parts.push(o.extensions.length === 1 ? o.extensions[0] : t('sidebar.packagesN', { n: o.extensions.length }));
    return parts.join(' · ');
  });
  const offerShown = $derived(!!upd.offer && app.piOfferKey !== app.piDismissed);
  const showUpd = $derived(offerShown || upd.running || !!upd.result);

  async function startUpdate() {
    const o = upd.offer;
    if (!o) return;
    const items = [...(o.pi ? [`pi ${o.pi[0]} → ${o.pi[1]}`] : []), ...o.extensions];
    const ok = await app.ask({
      title: S.updateAsk,
      body: S.updateAskBody,
      items,
      confirm: S.update,
    });
    if (ok) await app.runUpdate(o.pi && o.extensions.length ? 'all' : o.pi ? 'pi' : 'extensions');
  }

  const langName = $derived(LANGS.find((l) => l.code === i18n.lang)?.name ?? i18n.lang);
  /** The language list opens above the button; pressing the button again closes it. */
  function pickLang(e: MouseEvent) {
    if (menu.closedBy('lang')) return;
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    menu.show(r.left, r.top - 8 - LANGS.length * 36, LANGS.map((l) => ({ label: l.name, icon: l.code === i18n.lang ? 'check' : 'globe', run: () => setLang(l.code) } as MenuItem)), matchMedia('(pointer: coarse)').matches, 'lang');
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape' && !e.defaultPrevented && app.selecting && !app.dialog && !document.querySelector('dialog[open]')) app.stopSelect();
  }
</script>

<svelte:window onkeydown={onKey} />

<aside class:rail={app.rail}>
{#if app.rail}
  <div class="strip">
    <img class="mark" src="/logo.svg" alt="pi-oven" width="28" height="28" />
    <button class="icon-btn" type="button" aria-label={t('sidebar.unfold')} title={t('sidebar.unfold')} onclick={() => app.toggleSidebarFold()}><Icon name="sidebar" size={20} /></button>
    <button class="icon-btn open" type="button" aria-label={S.openFolder} title={S.openFolder} onclick={() => (app.folderPicker = true)}><Icon name="folder" size={20} /></button>
    <button class="icon-btn" type="button" aria-label={S.searchSessions} title={S.searchSessions} onclick={searchFromRail}><Icon name="search" size={20} /></button>
    <div class="chats" role="group" aria-label={t('sidebar.openChats')}>
      {#each app.chats as c (c.runtime.id)}
        {@const ttl = c.state?.sessionName || c.title || baseName(c.runtime.cwd)}
        <button class="chat" class:cur={c.runtime.id === app.currentId} type="button" title={showcase.tip('titles', `${ttl} (${baseName(c.runtime.cwd)})`)} aria-label={showcase.active('titles') ? t('sidebar.session') : ttl}
          aria-current={c.runtime.id === app.currentId ? 'true' : undefined} onclick={() => app.select(c.runtime.id)}>
          <span class="ch" data-priv="paths" aria-hidden="true">{Array.from(baseName(c.runtime.cwd))[0]?.toUpperCase() ?? '?'}</span>
          <span class="state" class:run={c.running} class:unseen={c.unseen} class:bg={!c.running && c.bg.running > 0} aria-hidden="true"></span>
        </button>
      {/each}
    </div>
    {#if offerShown || upd.running || app.ovenNotice}
      <button class="icon-btn upd-rail" class:busy={upd.running || app.ovenPhase === 'applying' || app.ovenPhase === 'restarting'} type="button" aria-label={S.updateAvailable} title="{S.updateAvailable}: {updSummary}"
        onclick={() => { app.toggleSidebarFold(); }}><Icon name="update" size={20} /></button>
    {/if}
    <button class="icon-btn" class:showing={showcase.on} type="button" aria-label={t('sidebar.settings')} title={showcase.on ? t('sidebar.settingsShowcase') : t('sidebar.settings')} onclick={() => app.openSettings(showcase.on ? 'showcase' : undefined)}><Icon name={showcase.on ? 'eyeOff' : 'gear'} size={20} /></button>
  </div>
{:else}
  <div class="brand">
    <img class="mark" src="/logo.svg" alt="" width="28" height="28" />
    <span class="word">pi-oven</span>
    {#if app.wide}
      <button class="icon-btn fold" type="button" aria-label={t('sidebar.fold')} title={t('sidebar.fold')} onclick={() => app.toggleSidebarFold()}><Icon name="sidebar" size={18} /></button>
    {/if}
    <button class="icon-btn close" type="button" aria-label={t('sidebar.closeList')} onclick={() => (app.sidebarOpen = false)}><Icon name="x" /></button>
  </div>

  <div class="actions">
    <button class="btn primary wide" type="button" onclick={() => (app.folderPicker = true)}>
      <Icon name="folder" size={16} />{S.openFolder}
    </button>
    <input class="field" type="search" bind:this={searchEl} placeholder={S.searchSessions} aria-label={S.searchSessions} bind:value={query} />
  </div>

  <nav aria-label={t('sidebar.sessions')}>
    {#each groups as g (g.cwd)}
      {@const all = !!query || app.selecting || isOpen(g.cwd)}
      {@const visible = all ? g.entries : g.entries.filter((e) => e.chat)}
      {@const hiddenN = g.entries.length - visible.length}
      {@const shown = expanded[g.cwd] || query || !all ? visible : visible.slice(0, LIMIT)}
      <section>
        <div class="gh" use:contextMenu={() => groupItems(g)}>
          {#if app.selecting}
            {@const st = groupState(g)}
            <button class="box" class:on={st !== 'none'} type="button" role="checkbox" aria-checked={st === 'all' ? 'true' : st === 'some' ? 'mixed' : 'false'}
              aria-label={t('sidebar.selectFolder', { folder: shortName(g.cwd) })} disabled={deletable(g).length === 0}
              onclick={() => app.pickMany(deletable(g), st !== 'all')}>
              {#if st === 'all'}<Icon name="check" size={12} />{:else if st === 'some'}<span class="dash"></span>{/if}
            </button>
          {/if}
          <button class="gt" type="button" title={showcase.tip('paths', g.cwd)} aria-expanded={all} disabled={!!query || app.selecting} onclick={() => toggleFold(g.cwd)}>
            <span class="gname" data-priv="paths"><span class="chev" class:open={all} aria-hidden="true"><Icon name="right" size={12} /></span>{baseName(g.cwd)}{#if hiddenN > 0}<span class="hidden-n" aria-label={t('sidebar.hiddenN', { n: hiddenN })}>{hiddenN}</span>{/if}</span>
            <span class="gpath mono" data-priv="paths">{tildePath(g.cwd, app.home)}</span>
          </button>
          <button class="icon-btn sm" class:hide={app.selecting} type="button" title="{S.newSession} ({baseName(g.cwd)})" aria-label={t('sidebar.newIn', { folder: baseName(g.cwd) })} onclick={() => app.openFolder(g.cwd)}>
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
                  <input class="rename" aria-label={t('sidebar.sessionName')} placeholder={t('sidebar.sessionName')} bind:value={draft} use:focusSelect
                    onkeydown={(ev) => { if (ev.isComposing) return; if (ev.key === 'Enter') { ev.preventDefault(); void commitRename(e); } else if (ev.key === 'Escape') { ev.preventDefault(); renaming = null; } }}
                    onblur={() => void commitRename(e)} />
                </div>
              {:else}
              <button class="row" class:selecting={app.selecting} class:locked type="button" onclick={() => open(e)} use:contextMenu={() => rowItems(g, e)}
                role={app.selecting ? 'checkbox' : undefined} aria-checked={app.selecting ? checked : undefined} aria-disabled={locked || undefined}
                title={locked ? t('sidebar.lockedHint') : undefined}
                aria-current={!app.selecting && e.chat && e.chat.runtime.id === app.currentId ? 'true' : undefined}>
                {#if app.selecting}
                  <span class="box" class:on={checked} aria-hidden="true">{#if checked}<Icon name="check" size={12} />{/if}</span>
                {:else}
                <span class="state" class:run={e.chat?.running} class:open={!!e.chat} class:unseen={e.chat?.unseen} class:bg={!e.chat?.running && (e.chat?.bg.running ?? 0) > 0} role="img" aria-label={e.chat?.running ? S.running : e.chat ? t('sidebar.opened') : ''}></span>
                {/if}
                <span class="t" data-priv="titles">{e.title}</span>
                <span class="m">{relativeTime(e.when)}</span>
              </button>
              {/if}
              {#if e.session && !app.selecting && renaming !== e.key}
                <button class="del icon-btn sm" type="button" aria-label={S.deleteSession} title={S.deleteSession} onclick={() => remove(e)}>
                  <Icon name="trash" size={15} />
                </button>
              {/if}
            </li>
          {/each}
        </ul>
        {#if !query && all && g.entries.length > LIMIT}
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
    <div class="selbar" role="toolbar" aria-label={t('sidebar.selbar')}>
      <span class="count" aria-live="polite">{t('sidebar.pickedN', { n: app.picked.length })}</span>
      <span class="grow"></span>
      <button class="btn quiet" type="button" onclick={() => app.pickMany(allSessions(), app.picked.length < allSessions().length)}>
        {app.picked.length < allSessions().length ? t('sidebar.selectAllShort') : t('sidebar.unselect')}
      </button>
      <button class="btn danger" type="button" disabled={app.picked.length === 0} onclick={deleteSelected}>
        <Icon name="trash" size={15} />{t('sidebar.delete')}
      </button>
      <button class="icon-btn" type="button" aria-label={t('sidebar.endSelect')} title={t('sidebar.endSelectEsc')} onclick={() => app.stopSelect()}><Icon name="x" size={18} /></button>
    </div>
  {/if}

  {#if app.ovenNotice || showUpd}
    <div class="cards">
      {#if app.ovenNotice}
        {#if app.ovenPhase === 'applying' || app.ovenPhase === 'restarting'}
          <UpdateCard state="busy" title={app.ovenPhase === 'applying' ? t('oven.applying') : t('oven.restarting')} detail={app.oven?.latest ?? ''} mono />
        {:else if app.ovenPhase === 'failed'}
          <UpdateCard state="bad" title={t('oven.failed')} detail={app.ovenError} onopen={() => app.openSettings('about')} onclose={() => (app.ovenPhase = 'idle')} />
        {:else if app.oven}
          <UpdateCard title={t('oven.newBuild')} detail={t('oven.cardDetail', { from: app.oven.current?.slice(0, 7) ?? '?', to: app.oven.latest?.slice(0, 7) ?? '' })} detailTitle="{app.oven.current ?? '?'} → {app.oven.latest}" mono onopen={() => app.openSettings('about')}
            action={{ label: S.update, run: () => app.ovenUpdate() }} onclose={() => app.dismissOven()} />
        {/if}
      {/if}
      {#if showUpd}
        {#if upd.running}
          <UpdateCard state="busy" title={S.updating} detail={updSummary || 'pi update'} />
        {:else if upd.result && !upd.result.ok}
          <UpdateCard state="bad" title={S.updateFailed} action={upd.offer ? { label: S.retry, run: startUpdate, primary: false } : undefined} onclose={() => (app.updates.result = null)}>
            {#snippet extra()}<details class="out"><summary>{S.updateOutput}</summary><pre>{upd.result?.output || '—'}</pre></details>{/snippet}
          </UpdateCard>
        {:else if upd.result}
          <UpdateCard state="done" title={S.updated} detail={app.chats.length ? S.updatedRestart : S.updatedNew}
            action={app.chats.length ? { label: S.restartSessions, run: () => app.restartIdle(), icon: 'refresh' } : undefined} onclose={() => (app.updates.result = null)} />
        {:else if upd.offer && offerShown}
          <UpdateCard title={S.updateAvailable} detail={updSummary} detailTitle={[...(upd.offer.pi ? [`pi ${upd.offer.pi[0]} → ${upd.offer.pi[1]}`] : []), ...upd.offer.extensions].join('\n')}
            onopen={() => app.openSettings('about')} action={{ label: S.update, run: startUpdate }} onclose={() => app.dismissPi()} />
        {/if}
      {/if}
    </div>
  {/if}

  <footer>
    <button class="btn quiet set" type="button" onclick={() => (app.settingsOpen = true)}>
      <Icon name="gear" size={16} />{t('sidebar.settings')}
    </button>
    <button class="lang" type="button" aria-label={t('sidebar.language', { name: langName })} title={t('sidebar.language', { name: langName })} aria-haspopup="menu" onclick={pickLang}>
      <Icon name="globe" size={15} /><span>{i18n.lang.toUpperCase()}</span>
    </button>
    {#if showcase.on}
      <button class="showtag" type="button" title={t('sidebar.showcaseOn')} onclick={() => app.openSettings('showcase')}><Icon name="eyeOff" size={13} />{t('sidebar.showcase')}</button>
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
    text-align: left;
    padding: 2px 4px;
    margin-left: -4px;
    border-radius: 6px;
    color: inherit;
  }
  .gt:disabled {
    cursor: default;
  }
  @media (hover: hover) {
    .gt:not(:disabled):hover {
      background: var(--paper-sunk);
    }
  }
  .chev {
    display: inline-grid;
    place-items: center;
    width: 14px;
    margin-right: 3px;
    vertical-align: -1px;
    color: var(--ink-3);
    transition: transform 0.15s;
  }
  .chev.open {
    transform: rotate(90deg);
  }
  .hidden-n {
    margin-left: 6px;
    padding: 0 6px;
    border-radius: 999px;
    background: var(--rule);
    color: var(--ink-2);
    font-size: 11px;
    font-weight: 560;
    vertical-align: 1px;
  }
  @media (prefers-reduced-motion: reduce) {
    .chev {
      transition: none;
    }
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
    position: relative;
  }
  .state.bg::after {
    content: '';
    position: absolute;
    inset: 0;
    border-radius: inherit;
    box-shadow: 0 0 0 4px color-mix(in srgb, var(--heat) 28%, transparent);
    opacity: 0;
    pointer-events: none;
    will-change: opacity;
    animation: ember 1.6s ease-in-out infinite;
  }
  .state.unseen {
    background: var(--accent);
  }
  .state.run {
    background: var(--heat);
    position: relative;
  }
  .state.run::after {
    content: '';
    position: absolute;
    inset: 0;
    border-radius: inherit;
    box-shadow: 0 0 0 4px color-mix(in srgb, var(--heat) 28%, transparent);
    opacity: 0;
    pointer-events: none;
    will-change: opacity;
    animation: ember 1.1s ease-in-out infinite;
  }
  /* the glow is a fixed ring that only fades (composited): animating box-shadow itself repaints every frame */
  @keyframes ember {
    50% {
      opacity: 1;
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
    flex: 1;
    justify-content: flex-start;
  }
  .lang {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 0 9px;
    height: 32px;
    border-radius: 8px;
    color: var(--ink-2);
    font-size: 12px;
    font-weight: 600;
    letter-spacing: 0.02em;
    flex: none;
  }
  @media (hover: hover) {
    .lang:hover {
      background: var(--rule);
      color: var(--ink);
    }
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
  .cards {
    display: grid;
    gap: 8px;
    margin: 0 10px 10px;
  }
  .out summary {
    font-size: 12px;
    color: var(--ink-3);
    cursor: pointer;
  }
  .out pre {
    margin: 6px 0 0;
    max-height: 160px;
    overflow: auto;
    font-family: var(--font-mono);
    font-size: 11px;
    white-space: pre-wrap;
    word-break: break-all;
    color: var(--ink-2);
  }
  .upd-rail {
    color: var(--accent);
  }
  .upd-rail.busy {
    color: var(--heat);
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
