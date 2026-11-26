<script lang="ts">
  import { api, HttpError } from '../lib/api';
  import { app } from '../lib/app.svelte';
  import { showcase } from '../lib/showcase.svelte';
  import type { Chat } from '../lib/chat.svelte';
  import type { SessionSummary } from '../lib/protocol';
  import { contextMenu, menu, type MenuItem } from '../lib/contextmenu.svelte';
  import { i18n, LANGS, setLang, t } from '../lib/i18n.svelte';
  import { S } from '../lib/strings';
  import { baseName, relativeTime, runClock, tildePath } from '../lib/util';
  import { locale } from '../lib/i18n';
  import { tick } from 'svelte';
  import Icon from './Icon.svelte';
  import UpdateCard from './UpdateCard.svelte';

  type Entry = { key: string; cwd: string; title: string; when: number; turns: number; session?: SessionSummary; chat?: Chat };
  type Day = { key: string; label: string; entries: Entry[] };
  type Tab = 'open' | 'history';

  let query = $state('');
  let searchEl = $state<HTMLInputElement>();
  /** The session whose name is being edited in its row. */
  let renaming = $state<string | null>(null);
  let draft = $state('');

  // Two lists: sessions with a pi running (open) and the ones only on disk (history). The tab is remembered here.
  const TAB_KEY = 'pi-oven.sidebar-tab';
  let tab = $state<Tab>((() => { try { return localStorage.getItem(TAB_KEY) === 'history' ? 'history' : 'open'; } catch { return 'open'; } })());
  function setTab(next: Tab) {
    tab = next;
    try { localStorage.setItem(TAB_KEY, next); } catch { /* private mode */ }
  }
  function tabKey(e: KeyboardEvent) {
    if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight' && e.key !== 'Home' && e.key !== 'End') return;
    e.preventDefault();
    const next: Tab = e.key === 'Home' ? 'open' : e.key === 'End' ? 'history' : tab === 'open' ? 'history' : 'open';
    setTab(next);
    (e.currentTarget as HTMLElement).parentElement?.querySelector<HTMLElement>(`[data-tab="${next}"]`)?.focus();
  }

  const entries = $derived.by<Entry[]>(() => {
    const q = query.trim().toLowerCase();
    const out: Entry[] = [];
    const claimed = new Set<Chat>();
    for (const s of app.sessions) {
      const chat = app.chatFor(s.path);
      if (chat) claimed.add(chat);
      out.push({ key: s.path, cwd: s.cwd, title: chat?.state?.sessionName || s.name || s.preview || S.untitled, when: s.modified, turns: s.turns, session: s, chat });
    }
    // Open sessions pi has not written to disk yet.
    for (const c of app.chats) {
      if (!claimed.has(c)) out.push({ key: c.runtime.id, cwd: c.runtime.cwd, title: c.state?.sessionName || c.title || S.newSession, when: Date.now(), turns: 0, chat: c });
    }
    return q ? out.filter((e) => `${e.title} ${e.cwd}`.toLowerCase().includes(q)) : out;
  });
  /** Where a session sits among the pins (-1: not pinned). Pinned ones stay in the Open tab, closed or not. */
  const pinAt = (e: Entry) => (e.session ? app.pinned.indexOf(e.session.path) : -1);
  // Pins first, in the order they were pinned; then open ones in the order they were opened, so a row does not jump
  // each time its session writes.
  const openList = $derived(
    entries
      .filter((e) => e.chat || pinAt(e) >= 0)
      .sort((a, b) => {
        const pa = pinAt(a), pb = pinAt(b);
        if (pa >= 0 || pb >= 0) return pa < 0 ? 1 : pb < 0 ? -1 : pa - pb;
        return app.chats.indexOf(a.chat!) - app.chats.indexOf(b.chat!);
      }),
  );
  const closedList = $derived(entries.filter((e) => !e.chat && pinAt(e) < 0).sort((a, b) => b.when - a.when));

  // History is long: it is drawn a page at a time.
  const PAGE = 80;
  let pages = $state(1);
  const days = $derived.by<Day[]>(() => {
    const list = query ? closedList : closedList.slice(0, pages * PAGE);
    const now = new Date();
    const today = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime();
    const DAY = 86_400_000;
    const bucket = (ms: number): [string, string] => {
      if (ms >= today) return ['today', t('sidebar.today')];
      if (ms >= today - DAY) return ['yesterday', t('sidebar.yesterday')];
      if (ms >= today - 6 * DAY) return ['week', t('sidebar.week')];
      if (ms >= today - 29 * DAY) return ['month', t('sidebar.month')];
      const d = new Date(ms);
      return [`${d.getFullYear()}-${d.getMonth()}`, d.toLocaleDateString(locale(), { year: 'numeric', month: 'long' })];
    };
    const out: Day[] = [];
    for (const e of list) {
      const [key, label] = bucket(e.when);
      if (out.at(-1)?.key === key) out.at(-1)!.entries.push(e);
      else out.push({ key, label, entries: [e] });
    }
    return out;
  });

  /** What an open session is doing, for the second line of its row. */
  const doing = (c: Chat) =>
    c.running ? t('sidebar.stRunning') : c.bg.running > 0 ? t('sidebar.stBg', { n: c.bg.running }) : c.unseen ? t('sidebar.stUnseen') : t('sidebar.stIdle');

  // A running session shows how long its run has taken; the clock ticks only while one is running.
  let now = $state(Date.now());
  const ticking = $derived(app.chats.some((c) => c.running && c.runSince != null));
  $effect(() => {
    if (!ticking) return;
    now = Date.now();
    const id = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(id);
  });

  const rowItems = (e: Entry): MenuItem[] => {
    const file = e.chat?.sessionFile ?? e.session?.path;
    if (app.selecting) {
      const on = !!e.session && app.picked.includes(e.session.path);
      return [
        { label: on ? t('sidebar.unselect') : t('sidebar.select'), icon: on ? 'x' : 'check', disabled: !e.session || !app.canDelete(e.session), run: () => e.session && app.togglePick(e.session) },
        { label: t('sidebar.selectAll'), icon: 'select', run: () => app.pickMany(allSessions(), true) },
        'sep',
        { label: t('sidebar.deletePicked', { n: app.picked.length }), icon: 'trash', danger: true, disabled: app.picked.length === 0, run: () => deleteSelected() },
        { label: t('sidebar.endSelect'), icon: 'x', run: () => app.stopSelect() },
      ];
    }
    return [
      { label: t('sidebar.open'), icon: 'chat', run: () => open(e) },
      { label: pinAt(e) >= 0 ? t('sidebar.unpin') : t('sidebar.pin'), icon: 'pin', disabled: !e.session, run: () => e.session && app.togglePin(e.session.path) },
      { label: S.rename, icon: 'edit', disabled: !e.chat && !e.session, run: () => startRename(e) },
      { label: t('sidebar.newIn', { folder: shortName(e.cwd) }), icon: 'plus', run: () => app.openFolder(e.cwd) },
      'sep',
      { label: t('sidebar.tree'), icon: 'fork', disabled: !file, run: () => file && (app.treeTarget = { path: file, cwd: e.cwd, title: e.title }) },
      { label: t('sidebar.move'), icon: 'folderOpen', disabled: !e.session, run: () => e.session && app.startMove(e.session) },
      'sep',
      { label: t('sidebar.copyFolder'), icon: 'folder', run: () => app.copy(e.cwd) },
      { label: t('sidebar.copyFile'), icon: 'link', disabled: !file, run: () => app.copy(file ?? '') },
      'sep',
      ...(e.chat ? [{ label: S.closeSession, icon: 'x', run: () => app.closeChat(e.chat!) } as MenuItem] : []),
      ...(e.session || e.chat ? [{ label: S.deleteSession, icon: 'trash', danger: true, run: () => remove(e) } as MenuItem] : []),
      ...(e.session ? [{ label: t('sidebar.selectToDelete'), icon: 'select', danger: true, run: () => app.startSelect(e.session) } as MenuItem] : []),
    ];
  };
  const dayItems = (d: Day): MenuItem[] => app.selecting
    ? [
        { label: t('sidebar.selectDay', { day: d.label }), icon: 'select', run: () => app.pickMany(deletable(d.entries), true) },
        { label: t('sidebar.unselectDay', { day: d.label }), icon: 'x', run: () => app.pickMany(deletable(d.entries), false) },
      ]
    : [{ label: t('sidebar.deleteDay', { day: d.label }), icon: 'select', danger: true, run: () => { app.startSelect(); app.pickMany(deletable(d.entries), true); } }];

  /** A folder name for a menu label: a long one keeps its start and end, so the rest of the label still shows. */
  const shortName = (cwd: string) => {
    const n = baseName(cwd);
    return n.length > 16 ? `${n.slice(0, 10)}…${n.slice(-5)}` : n;
  };
  const deletable = (list: Entry[]) => list.flatMap((e) => (e.session && app.canDelete(e.session) ? [e.session] : []));
  /** "All" while selecting is the list on screen: the other tab's sessions are not picked unseen. */
  const allSessions = () => deletable(tab === 'open' ? openList : closedList);
  const pickState = (list: Entry[]): 'all' | 'some' | 'none' => {
    const ok = deletable(list);
    const n = ok.filter((s) => app.picked.includes(s.path)).length;
    return n === 0 ? 'none' : n === ok.length ? 'all' : 'some';
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
    // open, but not in the list from disk (pi has not written it yet, or the list is behind)
    else if (e.chat) void app.deleteChat(e.chat);
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

  <div class="tabs" role="tablist" aria-label={t('sidebar.sessions')}>
    <button type="button" role="tab" data-tab="open" aria-selected={tab === 'open'} tabindex={tab === 'open' ? 0 : -1} class:on={tab === 'open'} onclick={() => setTab('open')} onkeydown={tabKey}>
      {t('sidebar.tabOpen')}<span class="n">{openList.length}</span>
    </button>
    <button type="button" role="tab" data-tab="history" aria-selected={tab === 'history'} tabindex={tab === 'history' ? 0 : -1} class:on={tab === 'history'} onclick={() => setTab('history')} onkeydown={tabKey}>
      {t('sidebar.tabHistory')}<span class="n">{closedList.length}</span>
    </button>
  </div>

  {#snippet row(e: Entry, open2: boolean)}
    {@const checked = !!e.session && app.picked.includes(e.session.path)}
    {@const locked = app.selecting && (!e.session || !app.canDelete(e.session))}
    {@const cur = !app.selecting && !!e.chat && e.chat.runtime.id === app.currentId}
    {@const pinned = pinAt(e) >= 0}
    <li class:active={cur} class:checked class:two={open2} class:closed={open2 && !e.chat}>
      {#if renaming === e.key}
        <div class="row editing">
          <span class="state" class:open={!!e.chat} aria-hidden="true"></span>
          <input class="rename" aria-label={t('sidebar.sessionName')} placeholder={t('sidebar.sessionName')} bind:value={draft} use:focusSelect
            onkeydown={(ev) => { if (ev.isComposing) return; if (ev.key === 'Enter') { ev.preventDefault(); void commitRename(e); } else if (ev.key === 'Escape') { ev.preventDefault(); renaming = null; } }}
            onblur={() => void commitRename(e)} />
        </div>
      {:else}
      <button class="row" class:selecting={app.selecting} class:locked type="button" onclick={() => open(e)} use:contextMenu={() => rowItems(e)}
        role={app.selecting ? 'checkbox' : undefined} aria-checked={app.selecting ? checked : undefined} aria-disabled={locked || undefined}
        title={locked ? t('sidebar.lockedHint') : open2 ? undefined : showcase.tip('paths', `${tildePath(e.cwd, app.home)} · ${relativeTime(e.when)}`)}
        aria-current={cur ? 'true' : undefined}>
        {#if app.selecting}
          <span class="box" class:on={checked} aria-hidden="true">{#if checked}<Icon name="check" size={12} />{/if}</span>
        {:else}
          <span class="state" class:run={e.chat?.running} class:open={!!e.chat} class:unseen={e.chat?.unseen} class:bg={!e.chat?.running && (e.chat?.bg.running ?? 0) > 0} role="img" aria-label={e.chat?.running ? S.running : e.chat ? t('sidebar.opened') : ''}></span>
        {/if}
        {#if open2}
          <span class="lines">
            <span class="t" data-priv="titles">{e.title}</span>
            <span class="sub"><span data-priv="paths">{baseName(e.cwd)}</span>{` · ${e.chat ? doing(e.chat) : t('sidebar.stClosed')}`}</span>
          </span>
          {#if e.chat?.running && e.chat.runSince != null}
            <span class="m clock">{runClock(Math.max(0, now - e.chat.runSince))}</span>
          {:else}
            <span class="m">{#if pinned}<span class="pin" role="img" aria-label={t('sidebar.pinned')} title={t('sidebar.pinned')}><Icon name="pin" size={12} /></span>{/if}{relativeTime(e.when)}</span>
          {/if}
        {:else}
          <span class="t" data-priv="titles">{e.title}</span>
          <span class="tag mono" data-priv="paths">{baseName(e.cwd)}</span>
        {/if}
      </button>
      {/if}
      {#if (e.session || e.chat) && !app.selecting && renaming !== e.key}
        <button class="del icon-btn sm" type="button" aria-label={S.deleteSession} title={S.deleteSession} onclick={() => remove(e)}>
          <Icon name="trash" size={15} />
        </button>
      {/if}
    </li>
  {/snippet}

  <nav aria-label={tab === 'open' ? t('sidebar.tabOpen') : t('sidebar.tabHistory')}>
    {#if tab === 'open'}
      {#if openList.length}
        <ul>{#each openList as e (e.key)}{@render row(e, true)}{/each}</ul>
      {:else}
        <p class="empty">{query ? S.noMatch : t('sidebar.noOpen')}</p>
      {/if}
    {:else}
      {#each days as d (d.key)}
        <section>
          <div class="day" use:contextMenu={() => dayItems(d)}>
            {#if app.selecting}
              {@const st = pickState(d.entries)}
              <button class="box" class:on={st !== 'none'} type="button" role="checkbox" aria-checked={st === 'all' ? 'true' : st === 'some' ? 'mixed' : 'false'}
                aria-label={t('sidebar.selectDay', { day: d.label })} disabled={deletable(d.entries).length === 0}
                onclick={() => app.pickMany(deletable(d.entries), st !== 'all')}>
                {#if st === 'all'}<Icon name="check" size={12} />{:else if st === 'some'}<span class="dash"></span>{/if}
              </button>
            {/if}
            <span>{d.label}</span>
          </div>
          <ul>{#each d.entries as e (e.key)}{@render row(e, false)}{/each}</ul>
        </section>
      {:else}
        <p class="empty">{query ? S.noMatch : t('sidebar.noHistory')}</p>
      {/each}
      {#if !query && closedList.length > pages * PAGE}
        <button class="more" type="button" onclick={() => pages++}>{S.showMore} ({closedList.length - pages * PAGE})</button>
      {/if}
    {/if}
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
  section + section {
    margin-top: 6px;
  }
  .tabs {
    display: flex;
    gap: 2px;
    margin: 0 14px 8px;
    padding: 2px;
    border: 1px solid var(--rule);
    border-radius: 8px;
  }
  .tabs button {
    flex: 1;
    height: 28px;
    border-radius: 6px;
    color: var(--ink-3);
    font-size: 13px;
    font-weight: 560;
  }
  .tabs button.on {
    background: var(--raised);
    color: var(--ink);
  }
  @media (hover: hover) {
    .tabs button:not(.on):hover {
      color: var(--ink-2);
    }
  }
  .tabs .n {
    margin-left: 5px;
    font-family: var(--font-mono);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
    color: var(--ink-3);
  }
  .day {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 10px 4px;
    font-size: 12px;
    color: var(--ink-3);
  }
  .lines {
    display: grid;
    min-width: 0;
    line-height: 1.3;
  }
  .sub {
    font-size: 12px;
    color: var(--ink-3);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tag {
    font-size: 11px;
    color: var(--ink-3);
    max-width: 88px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  li:not(.two) .t {
    color: var(--ink-2);
  }
  li.two .row {
    align-items: start;
  }
  li.two .state {
    margin-top: 6px;
  }
  /* closed: an empty square where an open one has its colour */
  li:not(.two) .row .state,
  li.closed .row .state {
    box-shadow: inset 0 0 0 1px var(--ink-3);
    opacity: 0.6;
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
  }
  .row {
    -webkit-touch-callout: none;
    user-select: none;
    width: 100%;
    display: grid;
    grid-template-columns: 8px minmax(0, 1fr) auto;
    align-items: center;
    gap: 8px;
    padding: 6px 8px;
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
  .m {
    font-size: 12px;
    color: var(--ink-3);
    font-variant-numeric: tabular-nums;
  }
  .m .pin {
    display: inline-flex;
    vertical-align: -1px;
    margin-right: 4px;
  }
  .m.clock {
    color: var(--heat);
    font-family: var(--font-mono);
    font-size: 11px;
  }
  .state {
    width: 8px;
    height: 8px;
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
  .state.unseen {
    background: var(--accent);
  }
  .state.run {
    background: var(--heat);
    position: relative;
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
    li:hover:has(.del) .m,
    li:hover:has(.del) .tag {
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
