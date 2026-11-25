<script lang="ts">
  import { S } from '../lib/strings';
  import { t } from '../lib/i18n.svelte';
  import { tick, untrack } from 'svelte';
  import { api, HttpError, type FileEntry } from '../lib/api';
  import { app } from '../lib/app.svelte';
  import { showcase } from '../lib/showcase.svelte';
  import type { Chat } from '../lib/chat.svelte';
  import { contextMenu, type MenuItem } from '../lib/contextmenu.svelte';
  import { FILE_MIME } from '../lib/editor.svelte';
  import { baseName } from '../lib/util';
  import FileIcon from './FileIcon.svelte';
  import Icon from './Icon.svelte';
  import LivePane from './LivePane.svelte';

  let { chat }: { chat: Chat } = $props();
  const id = $derived(chat.runtime.id);
  const KEY = $derived(`pi-oven.tree:${chat.runtime.cwd}`);

  interface Node { entries: FileEntry[]; loading: boolean; error: string | null; truncated: boolean }
  let dirs = $state<Record<string, Node>>({});
  let expanded = $state<string[]>([]);
  let selected = $state<string | null>(null);
  /** An inline name field: a new file or folder under `parent`, or a rename of `path`. */
  let editing = $state<{ mode: 'file' | 'dir' | 'rename'; parent: string; path?: string; value: string; busy: boolean } | null>(null);
  let box = $state<HTMLElement>();

  const join = (dir: string, name: string) => (dir ? `${dir}/${name}` : name);
  const parentOf = (p: string) => (p.includes('/') ? p.slice(0, p.lastIndexOf('/')) : '');

  async function load(path: string) {
    const prev = dirs[path];
    dirs[path] = { entries: prev?.entries ?? [], loading: true, error: null, truncated: prev?.truncated ?? false };
    try {
      const r = await api.files(id, path);
      dirs[path] = { entries: r.entries, loading: false, error: null, truncated: r.truncated };
    } catch (e) {
      dirs[path] = { entries: prev?.entries ?? [], loading: false, error: e instanceof HttpError && e.status === 404 ? t('files.noFolder') : t('files.readFailed'), truncated: false };
      // a folder that has gone is not worth keeping open
      if (e instanceof HttpError && e.status === 404) expanded = expanded.filter((x) => x !== path && !x.startsWith(path + '/'));
    }
  }

  const loadAll = () => Promise.all(['', ...expanded].map(load));

  // (reads the saved folders and loads them; only the folder's own key may re-run it, not what it sets)
  $effect(() => {
    const key = KEY;
    untrack(() => {
      try { expanded = JSON.parse(localStorage.getItem(key) ?? '[]'); } catch { expanded = []; }
      void loadAll();
    });
  });

  // pi ran something that writes files, or finished a turn: look again (once, after things settle).
  let timer: ReturnType<typeof setTimeout> | undefined;
  $effect(() => {
    chat.filesRev;
    untrack(() => {
      clearTimeout(timer);
      timer = setTimeout(() => void loadAll(), 700);
    });
    return () => clearTimeout(timer);
  });
  $effect(() => { if (!chat.running) untrack(() => void loadAll()); });

  // The folder watch names the folders to read again; together, a few times a second at most.
  let due: Set<string> | null = new Set();
  let dueTimer: ReturnType<typeof setTimeout> | undefined;
  $effect(() => {
    const live = chat.live;
    live.onDirs = (list) => {
      if (!list) due = null;
      else for (const d of list) due?.add(d);
      dueTimer ??= setTimeout(() => {
        dueTimer = undefined;
        const now = due;
        due = new Set();
        if (!now) void loadAll();
        else for (const d of now) if (dirs[d]) void load(d);
      }, 60);
    };
    return () => {
      live.onDirs = null;
      clearTimeout(dueTimer);
      dueTimer = undefined;
    };
  });

  // Following: the folders down to the file being written or changed open by themselves (not remembered as opened).
  const following = $derived(chat.live.follow && chat.running ? chat.live.target : null);
  $effect(() => {
    const p = following;
    chat.live.order;
    chat.live.n;
    if (!p || !inTree(p)) return;
    // (again when a folder on the way shows up)
    for (let i = p.indexOf('/'); i >= 0; i = p.indexOf('/', i + 1)) dirs[p.slice(0, i)]?.entries;
    dirs['']?.entries;
    untrack(() => void showPath(p));
  });
  /** Folded up by hand during this run: left so. */
  let shut = new Set<string>();
  $effect(() => { chat.running; shut = new Set(); });
  async function showPath(p: string) {
    const parts = p.split('/').slice(0, -1);
    for (let i = 0; i < parts.length; i++) {
      const dir = parts.slice(0, i + 1).join('/');
      const e = dirs[parts.slice(0, i).join('/')]?.entries.find((x) => x.name === parts[i]);
      if (!e || e.kind !== 'dir' || e.blocked || shut.has(dir)) return;
      if (!expanded.includes(dir)) expanded = [...expanded, dir];
      if (!dirs[dir]) await load(dir);
    }
    await tick();
    const row = box?.querySelector<HTMLElement>(`[data-path="${CSS.escape(p)}"]`);
    if (!row || !box || following !== p) return;
    // (only the tree scrolls: scrollIntoView would move the panel too)
    const r = row.getBoundingClientRect();
    const b = box.getBoundingClientRect();
    if (r.top < b.top) box.scrollTop -= b.top - r.top + 8;
    else if (r.bottom > b.bottom) box.scrollTop += r.bottom - b.bottom + 8;
  }

  function remember() {
    try { localStorage.setItem(KEY, JSON.stringify(expanded)); } catch { /* private mode */ }
  }

  async function toggle(path: string) {
    if (expanded.includes(path)) shut.add(path);
    else shut.delete(path);
    if (expanded.includes(path)) expanded = expanded.filter((x) => x !== path && !x.startsWith(path + '/'));
    else {
      expanded = [...expanded, path];
      if (!dirs[path]) await load(path);
      else void load(path);
    }
    remember();
  }

  async function reveal(path: string) {
    if (!expanded.includes(path)) {
      expanded = [...expanded, path];
      remember();
    }
    await load(path);
  }

  // ── rows ────────────────────────────────────────────────────────────────
  type Row =
    | { t: 'item'; path: string; name: string; e: FileEntry; depth: number; dir: boolean; open: boolean }
    | { t: 'draft'; path: string; name: string; depth: number; ask: boolean }
    | { t: 'edit'; depth: number; kind: 'file' | 'dir' | 'rename'; key: string };

  const rows = $derived.by<Row[]>(() => {
    const out: Row[] = [];
    const walk = (dir: string, depth: number) => {
      if (editing && editing.mode !== 'rename' && editing.parent === dir) out.push({ t: 'edit', depth, kind: editing.mode, key: 'new:' + dir });
      const names = new Set((dirs[dir]?.entries ?? []).map((e) => e.name));
      // files the model is writing that are not there yet
      if (dirs[dir]) for (const d of chat.live.drafts) if (parentOf(d.key) === dir && inTree(d.key) && !names.has(baseName(d.key)) && d.draft.kind === 'write') out.push({ t: 'draft', path: d.key, name: baseName(d.key), depth, ask: d.phase === 'ask' });
      for (const e of dirs[dir]?.entries ?? []) {
        const path = join(dir, e.name);
        const isDir = e.kind === 'dir';
        const open = isDir && expanded.includes(path);
        if (editing?.mode === 'rename' && editing.path === path) out.push({ t: 'edit', depth, kind: 'rename', key: 'rename:' + path });
        else out.push({ t: 'item', path, name: e.name, e, depth, dir: isDir, open });
        if (open && !e.blocked) walk(path, depth + 1);
      }
    };
    walk('', 0);
    return out;
  });

  const inTree = (p: string) => !p.startsWith('/') && !p.startsWith('~');
  /** What is going on inside a folder that is folded up. */
  function inside(dir: string) {
    const pre = dir + '/';
    let n = 0;
    let fresh = true;
    for (const [p, m] of Object.entries(chat.live.marks)) if (p.startsWith(pre)) { n++; if (m !== 'new') fresh = false; }
    const busy = chat.live.drafts.some((d) => d.key.startsWith(pre));
    return n || busy ? { n, busy, mark: fresh ? 'new' : 'mod' } : null;
  }

  // ── how the height is shared with the live changes ──────────────────────
  let frac = $state(readFrac());
  let tab = $state<HTMLElement>();
  function readFrac() {
    try { const v = Number(localStorage.getItem('pi-oven.live-frac')); return v >= 0.2 && v <= 0.8 ? v : 0.54; } catch { return 0.54; }
  }
  function setFrac(v: number, keep = false) {
    frac = Math.min(0.8, Math.max(0.2, v));
    if (keep) try { localStorage.setItem('pi-oven.live-frac', String(frac)); } catch { /* private mode */ }
  }
  function grip(e: PointerEvent) {
    const el = e.currentTarget as HTMLElement;
    el.setPointerCapture(e.pointerId);
    const move = (m: PointerEvent) => {
      const r = tab!.getBoundingClientRect();
      setFrac((r.bottom - m.clientY) / r.height);
    };
    const up = () => {
      el.removeEventListener('pointermove', move);
      el.removeEventListener('pointerup', up);
      el.removeEventListener('pointercancel', up);
      setFrac(frac, true);
    };
    el.addEventListener('pointermove', move);
    el.addEventListener('pointerup', up);
    el.addEventListener('pointercancel', up);
  }
  function gripKey(e: KeyboardEvent) {
    const d = e.key === 'ArrowUp' ? 0.04 : e.key === 'ArrowDown' ? -0.04 : 0;
    if (!d) return;
    e.preventDefault();
    setFrac(frac + d, true);
  }

  const openPaths = $derived(new Set(chat.editors.docs.map((d) => d.path)));

  // ── actions ─────────────────────────────────────────────────────────────
  function openFile(path: string) {
    chat.live.seen(path);
    void chat.editors.open(path);
    if (app.narrow) app.inspectorDrawer = false;
  }

  function click(r: Extract<Row, { t: 'item' }>) {
    selected = r.path;
    if (r.e.blocked) return;
    if (r.dir) void toggle(r.path);
    else openFile(r.path);
  }

  function startNew(mode: 'file' | 'dir', parent: string) {
    void reveal(parent).then(() => (editing = { mode, parent, value: '', busy: false }));
    editing = { mode, parent, value: '', busy: false };
  }

  function startRename(path: string) {
    editing = { mode: 'rename', parent: parentOf(path), path, value: baseName(path), busy: false };
  }

  const bad = (v: string) => !v.trim() || v.includes('/') || v === '.' || v === '..';

  async function commit() {
    const ed = editing;
    if (!ed || ed.busy) return;
    const name = ed.value.trim();
    if (!name) { editing = null; return; }
    if (bad(name)) { app.flash(t('files.noSlash')); return; }
    ed.busy = true;
    try {
      if (ed.mode === 'rename') {
        const from = ed.path!;
        const to = join(ed.parent, name);
        if (to !== from) {
          chat.editors.ownChange(to);
          await api.renameFile(id, from, to);
          chat.editors.renamed(from, to);
          expanded = expanded.map((x) => (x === from || x.startsWith(from + '/') ? to + x.slice(from.length) : x));
          remember();
          if (selected === from) selected = to;
        }
        editing = null;
        await Promise.all([load(ed.parent), ...(to !== from ? expanded.filter((x) => x === to || x.startsWith(to + '/')).map(load) : [])]);
      } else {
        const path = join(ed.parent, name);
        chat.editors.ownChange(path);
        await api.createFile(id, path, ed.mode);
        editing = null;
        selected = path;
        await load(ed.parent);
        if (ed.mode === 'file') openFile(path);
        else await reveal(path);
      }
    } catch (e) {
      ed.busy = false;
      app.flash(e instanceof HttpError && e.status === 409 ? t('files.exists') : e instanceof HttpError && e.status === 403 ? t('files.forbidden') : t('files.failed'));
    }
  }

  async function remove(path: string, dir: boolean) {
    const ok = await app.ask({
      title: dir ? t('files.delDirAsk') : t('files.delFileAsk'),
      body: dir ? t('files.delDirBody') : t('files.delFileBody'),
      items: [path],
      confirm: t('files.delete'),
      danger: true,
    });
    if (!ok) return;
    try {
      await api.deleteFile(id, path);
      chat.editors.gone(path);
      expanded = expanded.filter((x) => x !== path && !x.startsWith(path + '/'));
      remember();
      if (selected === path) selected = null;
      await load(parentOf(path));
    } catch {
      app.flash(t('files.delFailed'));
    }
  }

  function itemsFor(r: Extract<Row, { t: 'item' }>): MenuItem[] {
    const where = r.dir ? r.path : parentOf(r.path);
    return [
      ...(r.dir ? [] : [{ label: t('files.openEditor'), icon: 'file', disabled: r.e.blocked, run: () => openFile(r.path) } as MenuItem]),
      ...(r.dir ? [{ label: r.open ? t('files.collapse') : t('files.expand'), icon: r.open ? 'up' : 'down', run: () => void toggle(r.path) } as MenuItem] : []),
      'sep',
      { label: t('files.newFileMenu'), icon: 'filePlus', run: () => startNew('file', where) },
      { label: t('files.newDirMenu'), icon: 'folderPlus', run: () => startNew('dir', where) },
      'sep',
      { label: S.rename, icon: 'edit', run: () => startRename(r.path) },
      { label: t('files.deleteMenu'), icon: 'trash', danger: true, run: () => void remove(r.path, r.dir) },
      'sep',
      { label: t('files.copyPath'), icon: 'link', run: () => app.copy(r.path) },
      { label: t('files.copyFullPath'), icon: 'link', run: () => app.copy(`${chat.runtime.cwd}/${r.path}`) },
      ...(r.dir ? [{ label: S.refresh, icon: 'refresh', run: () => void load(r.path) } as MenuItem] : []),
    ];
  }

  const rootItems = (): MenuItem[] => [
    { label: t('files.newFileMenu'), icon: 'filePlus', run: () => startNew('file', '') },
    { label: t('files.newDirMenu'), icon: 'folderPlus', run: () => startNew('dir', '') },
    'sep',
    { label: S.refresh, icon: 'refresh', run: () => void loadAll() },
    ...(Object.keys(chat.live.marks).length ? [{ label: t('live.clear'), icon: 'check', run: () => chat.live.clearMarks() } as MenuItem] : []),
  ];

  function dragStart(e: DragEvent, path: string) {
    e.dataTransfer?.setData(FILE_MIME, JSON.stringify({ path, runtime: chat.runtime.id }));
    e.dataTransfer?.setData('text/plain', path);
    if (e.dataTransfer) e.dataTransfer.effectAllowed = 'copy';
  }

  // ── keyboard ────────────────────────────────────────────────────────────
  function focusRow(path: string) {
    selected = path;
    queueMicrotask(() => box?.querySelector<HTMLElement>(`[data-path="${CSS.escape(path)}"]`)?.focus());
  }

  function onKey(e: KeyboardEvent) {
    if (editing || (e.target as HTMLElement).tagName === 'INPUT') return;
    const items = rows.filter((r): r is Extract<Row, { t: 'item' }> => r.t === 'item');
    const i = items.findIndex((r) => r.path === selected);
    const cur = items[i];
    switch (e.key) {
      case 'ArrowDown': e.preventDefault(); if (items[i + 1]) focusRow(items[i + 1].path); else if (i < 0 && items[0]) focusRow(items[0].path); break;
      case 'ArrowUp': e.preventDefault(); if (i > 0) focusRow(items[i - 1].path); break;
      case 'ArrowRight': if (cur?.dir && !cur.open) { e.preventDefault(); void toggle(cur.path); } else if (cur?.dir && items[i + 1]) { e.preventDefault(); focusRow(items[i + 1].path); } break;
      case 'ArrowLeft': if (cur?.dir && cur.open) { e.preventDefault(); void toggle(cur.path); } else if (cur) { const p = parentOf(cur.path); if (p) { e.preventDefault(); focusRow(p); } } break;
      case 'Enter': if (cur) { e.preventDefault(); click(cur); } break;
      case 'F2': if (cur) { e.preventDefault(); startRename(cur.path); } break;
      case 'Delete': if (cur) { e.preventDefault(); void remove(cur.path, cur.dir); } break;
    }
  }

  const stem = (node: HTMLInputElement) => {
    node.focus();
    const v = node.value;
    const dot = v.lastIndexOf('.');
    node.setSelectionRange(0, dot > 0 ? dot : v.length);
  };

  const root = $derived(dirs['']);
  const targetDir = $derived.by(() => {
    if (!selected) return '';
    const e = rows.find((r) => r.t === 'item' && r.path === selected) as Extract<Row, { t: 'item' }> | undefined;
    return e?.dir ? e.path : parentOf(selected);
  });
</script>

<div class="tab" bind:this={tab}>
  <div class="bar">
    <span class="root mono" title={showcase.tip('paths', chat.runtime.cwd)} data-priv="paths">{baseName(chat.runtime.cwd)}</span>
    <span class="acts">
      <button class="icon-btn sm" type="button" title={t('files.newFile')} aria-label={t('files.newFile')} onclick={() => startNew('file', targetDir)}><Icon name="filePlus" size={16} /></button>
      <button class="icon-btn sm" type="button" title={t('files.newDir')} aria-label={t('files.newDir')} onclick={() => startNew('dir', targetDir)}><Icon name="folderPlus" size={16} /></button>
      <button class="icon-btn sm" type="button" title={S.refresh} aria-label={S.refresh} onclick={() => void loadAll()}><Icon name="refresh" size={15} /></button>
    </span>
  </div>

  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="tree" role="tree" aria-label={t('files.tree')} tabindex="-1" bind:this={box} onkeydown={onKey} use:contextMenu={(e) => ((e.target as HTMLElement).closest('[data-path]') ? null : rootItems())}>
    {#if root?.error}<p class="err" role="alert">{root.error}</p>{/if}
    {#if root && !root.loading && root.entries.length === 0 && !editing && !root.error}
      <p class="note">{t('files.empty')}</p>
    {/if}
    {#each rows as r (r.t === 'edit' ? r.key : r.path)}
      {#if r.t === 'draft'}
        <div class="row draft" style="--d:{r.depth}" role="treeitem" aria-selected="false">
          <button type="button" class="hit" data-path={r.path} tabindex="-1" onclick={() => chat.live.pick(r.path)} title={showcase.tip('files', r.path)}>
            <span class="chev"></span>
            <span class="ic" aria-hidden="true"><FileIcon name={r.name} /></span>
            <span class="nm" data-priv="files">{r.name}</span>
            <span class="end"><span class="tag">{r.ask ? t('live.askTag') : t('live.draftTag')}</span><i class="sq run" aria-hidden="true"></i></span>
          </button>
        </div>
      {:else if r.t === 'item'}
        {@const draft = r.dir ? undefined : chat.live.draftOf(r.path)}
        {@const mark = chat.live.marks[r.path]}
        {@const sum = r.dir && !r.open ? inside(r.path) : null}
        {@const ch = mark ? chat.live.changes.get(r.path) : undefined}
        <div class="row" class:run={!!draft} class:new={mark === 'new'} class:mod={mark === 'mod'} class:born={chat.live.fresh[r.path] && mark === 'new'} class:saved={chat.live.fresh[r.path] && mark === 'mod'} class:sel={selected === r.path} class:open-doc={openPaths.has(r.path)} class:active={chat.editors.active === r.path} class:blocked={r.e.blocked}
          style="--d:{r.depth}" role="treeitem" aria-selected={selected === r.path} aria-expanded={r.dir ? r.open : undefined}
          use:contextMenu={() => itemsFor(r)}>
          <button type="button" class="hit" data-path={r.path} tabindex={selected === r.path || (!selected && r.path === rows.find((x) => x.t === 'item')?.['path' as never]) ? 0 : -1}
            draggable={!r.dir && !r.e.blocked} ondragstart={(e) => dragStart(e, r.path)} onclick={() => click(r)} ondblclick={() => (!r.dir && !r.e.blocked ? openFile(r.path) : null)}
            title={r.e.blocked ? t('files.blocked') : showcase.tip('files', r.path)}>
            <span class="chev" class:up={r.open} aria-hidden="true">{#if r.dir}<Icon name="right" size={13} />{/if}</span>
            <span class="ic" class:dot={r.name.startsWith('.')} aria-hidden="true">{#if r.dir}<Icon name={r.open ? 'folderOpen' : 'folder'} size={15} />{:else}<FileIcon name={r.name} />{/if}</span>
            <span class="nm" data-priv="files">{r.name}</span>
            {#if r.e.link}<span class="lk" aria-label={t('files.link')}>↗</span>{/if}
            <span class="end">
              {#if draft}<span class="tag">{draft.phase === 'ask' ? t('live.askTag') : t('live.draftTag')}</span><i class="sq run" aria-hidden="true"></i>
              {:else if mark}
                {#if ch}<span class="num">{#if ch.added}<b class="p">+{ch.added}</b>{/if}{#if ch.removed} <b class="m">−{ch.removed}</b>{/if}</span>{/if}
                <i class="sq {mark}" role="img" aria-label={mark === 'new' ? t('live.new') : t('live.modified')}></i>
              {:else if sum}
                {#if sum.n}<span class="cnt" title={t('live.marks', { n: sum.n })}>{sum.n}</span>{/if}<i class="sq {sum.busy ? 'run' : sum.mark}" aria-hidden="true"></i>
              {/if}
              {#if openPaths.has(r.path)}<i class="od" role="img" aria-label={t('files.isOpen')}></i>{/if}
            </span>
          </button>
        </div>
        {#if r.dir && r.open && dirs[r.path]?.loading && !dirs[r.path]?.entries.length}
          <p class="note" style="--d:{r.depth + 1}">{S.loading}</p>
        {:else if r.dir && r.open && dirs[r.path]?.error}
          <p class="err" style="--d:{r.depth + 1}">{dirs[r.path].error}</p>
        {:else if r.dir && r.open && dirs[r.path]?.truncated}
          <p class="note" style="--d:{r.depth + 1}">{t('files.truncated')}</p>
        {/if}
      {:else}
        <div class="row editing" style="--d:{r.depth}">
          <span class="hit">
            <span class="chev"></span>
            <span class="ic" aria-hidden="true">{#if r.kind === 'dir'}<Icon name="folder" size={15} />{:else if editing && editing.value}<FileIcon name={editing.value} />{:else}<Icon name="file" size={15} />{/if}</span>
            {#if editing}
              <input class="name" aria-label={r.kind === 'rename' ? t('files.newName') : r.kind === 'dir' ? t('files.newDirName') : t('files.newFileName')} bind:value={editing.value} use:stem disabled={editing.busy}
                placeholder={r.kind === 'dir' ? t('files.dirName') : t('files.fileName')}
                onkeydown={(e) => { if (e.isComposing) return; if (e.key === 'Enter') { e.preventDefault(); void commit(); } else if (e.key === 'Escape') { e.preventDefault(); editing = null; } }}
                onblur={() => { if (editing && !editing.busy) { editing.value.trim() ? void commit() : (editing = null); } }} />
            {/if}
          </span>
        </div>
      {/if}
    {/each}
    {#if root?.truncated}<p class="note">{t('files.truncated')}</p>{/if}
  </div>
  {#if chat.live.active}
    <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
    <div class="grip" role="separator" aria-orientation="horizontal" aria-label={t('live.grip')} aria-valuenow={Math.round(frac * 100)} aria-valuemin="20" aria-valuemax="80" tabindex="0" onpointerdown={grip} onkeydown={gripKey}></div>
    <div class="low" style="flex-basis:{frac * 100}%"><LivePane {chat} /></div>
  {:else}
    <p class="hint">{t('files.hint')}</p>
  {/if}
</div>

<style>
  .tab { flex: 1; min-height: 0; min-width: 0; display: flex; flex-direction: column; }
  .bar { flex: none; padding: 14px 16px 8px; display: flex; align-items: center; justify-content: space-between; gap: 8px; min-width: 0; }
  .bar .acts { flex: none; }
  .root { min-width: 0; font-size: 12.5px; font-weight: 650; color: var(--ink-2); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .acts { display: flex; flex: none; }
  .sm { width: 28px; height: 28px; }
  .tree { display: grid; grid-template-columns: minmax(0, 1fr); align-content: start; min-width: 0; flex: 1 1 0; min-height: 80px; overflow-y: auto; overscroll-behavior: contain; padding: 0 10px 10px; }
  .note, .err { margin: 2px 0; padding-left: calc(30px + var(--d, 0) * 14px); font-size: 12.5px; color: var(--ink-3); }
  .err { color: var(--bad); }
  .grip { flex: none; height: 11px; display: grid; place-items: center; border-top: 1px solid var(--rule); cursor: row-resize; touch-action: none; }
  .grip::after { content: ''; width: 34px; height: 3px; border-radius: 2px; background: var(--rule); }
  .grip:focus-visible { outline: none; }
  .grip:focus-visible::after { background: var(--accent); }
  .low { flex: 0 0 auto; min-height: 120px; display: flex; }
  .end { margin-left: auto; flex: none; display: inline-flex; align-items: center; gap: 7px; padding-left: 6px; }
  .end:empty { display: none; }
  .tag { font-size: 11.5px; color: var(--ink-3); font-style: normal; }
  .cnt, .num { font: 11.5px var(--font-mono); font-variant-numeric: tabular-nums; color: var(--ink-3); }
  .num b { font-weight: 500; }
  .num .p { color: var(--ok); }
  .num .m { color: var(--bad); }
  .sq { flex: none; width: 8px; height: 8px; border-radius: 2px; }
  .sq.run { background: var(--heat); }
  .sq.new { background: var(--ok); }
  .sq.mod { box-shadow: inset 0 0 0 1.5px var(--accent); }
  .row.new .nm { color: var(--ok); }
  .row.mod .nm, .row.run .nm { color: var(--ink); }
  .row.draft .nm { color: var(--ink-3); font-style: italic; }
  .row.born { animation: born 0.42s cubic-bezier(0.3, 1.5, 0.5, 1); }
  .row.saved { animation: saved 0.52s cubic-bezier(0.3, 1.5, 0.5, 1); }
  @keyframes born { from { transform: translateX(-10px); opacity: 0; } }
  @keyframes saved { 40% { transform: translateX(5px); } }
  @media (prefers-reduced-motion: reduce) { .row.born, .row.saved { animation: none; } }
  .hint { flex: none; margin: 4px 16px 14px; font-size: 12px; color: var(--ink-3); }
  .row { min-width: 0; border-radius: 6px; }
  .hit {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 4px;
    min-width: 0;
    height: 28px;
    padding: 0 8px 0 calc(6px + var(--d) * 14px);
    border-radius: 6px;
    text-align: left;
    color: var(--ink-2);
    font-size: 13px;
  }
  .chev { flex: none; width: 14px; display: inline-grid; place-items: center; color: var(--ink-3); transition: rotate 0.1s; }
  .chev.up { rotate: 90deg; }
  .ic { flex: none; display: inline-grid; color: var(--ink-3); }
  .ic.dot { opacity: 0.6; }
  .nm { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .lk { flex: none; color: var(--ink-3); font-size: 11px; }
  .od { flex: none; width: 6px; height: 6px; border-radius: 50%; background: var(--accent); }
  .row.sel .hit { background: color-mix(in srgb, var(--rule) 55%, transparent); color: var(--ink); }
  .row.active .hit { color: var(--ink); font-weight: 620; }
  .row.blocked .hit { opacity: 0.5; cursor: default; }
  .row:not(.editing) .hit:focus-visible { outline: 2px solid color-mix(in srgb, var(--accent) 60%, transparent); outline-offset: -2px; }
  @media (hover: hover) {
    .row:not(.sel):not(.editing) .hit:hover { background: color-mix(in srgb, var(--rule) 35%, transparent); }
  }
  .name {
    flex: 1;
    min-width: 0;
    height: 24px;
    padding: 0 6px;
    border-radius: 5px;
    border: 1px solid var(--accent);
    background: var(--raised);
    color: var(--ink);
    font: inherit;
    font-size: 13px;
  }
  .name:focus { outline: none; }
</style>
