<script lang="ts">
  import { untrack } from 'svelte';
  import { api, HttpError, type FileEntry } from '../lib/api';
  import { app } from '../lib/app.svelte';
  import { showcase } from '../lib/showcase.svelte';
  import type { Chat } from '../lib/chat.svelte';
  import { contextMenu, type MenuItem } from '../lib/contextmenu.svelte';
  import { FILE_MIME } from '../lib/editor.svelte';
  import { baseName } from '../lib/util';
  import FileIcon from './FileIcon.svelte';
  import Icon from './Icon.svelte';

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
      dirs[path] = { entries: prev?.entries ?? [], loading: false, error: e instanceof HttpError && e.status === 404 ? '폴더가 없습니다.' : '폴더를 읽지 못했습니다.', truncated: false };
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

  function remember() {
    try { localStorage.setItem(KEY, JSON.stringify(expanded)); } catch { /* private mode */ }
  }

  async function toggle(path: string) {
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
    | { t: 'edit'; depth: number; kind: 'file' | 'dir' | 'rename'; key: string };

  const rows = $derived.by<Row[]>(() => {
    const out: Row[] = [];
    const walk = (dir: string, depth: number) => {
      if (editing && editing.mode !== 'rename' && editing.parent === dir) out.push({ t: 'edit', depth, kind: editing.mode, key: 'new:' + dir });
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

  const openPaths = $derived(new Set(chat.editors.docs.map((d) => d.path)));

  // ── actions ─────────────────────────────────────────────────────────────
  function openFile(path: string) {
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
    if (bad(name)) { app.flash('이름에는 / 를 쓸 수 없습니다'); return; }
    ed.busy = true;
    try {
      if (ed.mode === 'rename') {
        const from = ed.path!;
        const to = join(ed.parent, name);
        if (to !== from) {
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
        await api.createFile(id, path, ed.mode);
        editing = null;
        selected = path;
        await load(ed.parent);
        if (ed.mode === 'file') openFile(path);
        else await reveal(path);
      }
    } catch (e) {
      ed.busy = false;
      app.flash(e instanceof HttpError && e.status === 409 ? '같은 이름이 이미 있습니다' : e instanceof HttpError && e.status === 403 ? '여기에는 만들 수 없습니다' : '처리하지 못했습니다');
    }
  }

  async function remove(path: string, dir: boolean) {
    const ok = await app.ask({
      title: dir ? '폴더를 삭제할까요?' : '파일을 삭제할까요?',
      body: dir ? '폴더 안의 모든 파일이 함께 영구 삭제되며 되돌릴 수 없습니다.' : '영구 삭제되며 되돌릴 수 없습니다.',
      items: [path],
      confirm: '삭제',
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
      app.flash('삭제하지 못했습니다');
    }
  }

  function itemsFor(r: Extract<Row, { t: 'item' }>): MenuItem[] {
    const where = r.dir ? r.path : parentOf(r.path);
    return [
      ...(r.dir ? [] : [{ label: '편집기로 열기', icon: 'file', disabled: r.e.blocked, run: () => openFile(r.path) } as MenuItem]),
      ...(r.dir ? [{ label: r.open ? '접기' : '펼치기', icon: r.open ? 'up' : 'down', run: () => void toggle(r.path) } as MenuItem] : []),
      'sep',
      { label: '새 파일…', icon: 'filePlus', run: () => startNew('file', where) },
      { label: '새 폴더…', icon: 'folderPlus', run: () => startNew('dir', where) },
      'sep',
      { label: '이름 바꾸기', icon: 'edit', run: () => startRename(r.path) },
      { label: '삭제…', icon: 'trash', danger: true, run: () => void remove(r.path, r.dir) },
      'sep',
      { label: '경로 복사', icon: 'link', run: () => app.copy(r.path) },
      { label: '전체 경로 복사', icon: 'link', run: () => app.copy(`${chat.runtime.cwd}/${r.path}`) },
      ...(r.dir ? [{ label: '새로 고침', icon: 'refresh', run: () => void load(r.path) } as MenuItem] : []),
    ];
  }

  const rootItems = (): MenuItem[] => [
    { label: '새 파일…', icon: 'filePlus', run: () => startNew('file', '') },
    { label: '새 폴더…', icon: 'folderPlus', run: () => startNew('dir', '') },
    'sep',
    { label: '새로 고침', icon: 'refresh', run: () => void loadAll() },
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

<div class="tab">
  <div class="bar">
    <span class="root mono" title={showcase.tip('paths', chat.runtime.cwd)} data-priv="paths">{baseName(chat.runtime.cwd)}</span>
    <span class="acts">
      <button class="icon-btn sm" type="button" title="새 파일" aria-label="새 파일" onclick={() => startNew('file', targetDir)}><Icon name="filePlus" size={16} /></button>
      <button class="icon-btn sm" type="button" title="새 폴더" aria-label="새 폴더" onclick={() => startNew('dir', targetDir)}><Icon name="folderPlus" size={16} /></button>
      <button class="icon-btn sm" type="button" title="새로 고침" aria-label="새로 고침" onclick={() => void loadAll()}><Icon name="refresh" size={15} /></button>
    </span>
  </div>

  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="tree" role="tree" aria-label="파일" tabindex="-1" bind:this={box} onkeydown={onKey} use:contextMenu={(e) => ((e.target as HTMLElement).closest('[data-path]') ? null : rootItems())}>
    {#if root?.error}<p class="err" role="alert">{root.error}</p>{/if}
    {#if root && !root.loading && root.entries.length === 0 && !editing && !root.error}
      <p class="note">비어 있는 폴더입니다. 위의 + 로 새 파일을 만들 수 있어요.</p>
    {/if}
    {#each rows as r (r.t === 'item' ? r.path : r.key)}
      {#if r.t === 'item'}
        <div class="row" class:sel={selected === r.path} class:open-doc={openPaths.has(r.path)} class:active={chat.editors.active === r.path} class:blocked={r.e.blocked}
          style="--d:{r.depth}" role="treeitem" aria-selected={selected === r.path} aria-expanded={r.dir ? r.open : undefined}
          use:contextMenu={() => itemsFor(r)}>
          <button type="button" class="hit" data-path={r.path} tabindex={selected === r.path || (!selected && r.path === rows.find((x) => x.t === 'item')?.['path' as never]) ? 0 : -1}
            draggable={!r.dir && !r.e.blocked} ondragstart={(e) => dragStart(e, r.path)} onclick={() => click(r)} ondblclick={() => (!r.dir && !r.e.blocked ? openFile(r.path) : null)}
            title={r.e.blocked ? '폴더 밖을 가리키는 링크라 열 수 없습니다' : showcase.tip('files', r.path)}>
            <span class="chev" class:up={r.open} aria-hidden="true">{#if r.dir}<Icon name="right" size={13} />{/if}</span>
            <span class="ic" class:dot={r.name.startsWith('.')} aria-hidden="true">{#if r.dir}<Icon name={r.open ? 'folderOpen' : 'folder'} size={15} />{:else}<FileIcon name={r.name} />{/if}</span>
            <span class="nm" data-priv="files">{r.name}</span>
            {#if r.e.link}<span class="lk" aria-label="링크">↗</span>{/if}
            {#if openPaths.has(r.path)}<i class="od" role="img" aria-label="열려 있음"></i>{/if}
          </button>
        </div>
        {#if r.dir && r.open && dirs[r.path]?.loading && !dirs[r.path]?.entries.length}
          <p class="note" style="--d:{r.depth + 1}">불러오는 중…</p>
        {:else if r.dir && r.open && dirs[r.path]?.error}
          <p class="err" style="--d:{r.depth + 1}">{dirs[r.path].error}</p>
        {:else if r.dir && r.open && dirs[r.path]?.truncated}
          <p class="note" style="--d:{r.depth + 1}">항목이 많아 일부만 표시합니다.</p>
        {/if}
      {:else}
        <div class="row editing" style="--d:{r.depth}">
          <span class="hit">
            <span class="chev"></span>
            <span class="ic" aria-hidden="true">{#if r.kind === 'dir'}<Icon name="folder" size={15} />{:else if editing && editing.value}<FileIcon name={editing.value} />{:else}<Icon name="file" size={15} />{/if}</span>
            {#if editing}
              <input class="name" aria-label={r.kind === 'rename' ? '새 이름' : r.kind === 'dir' ? '새 폴더 이름' : '새 파일 이름'} bind:value={editing.value} use:stem disabled={editing.busy}
                placeholder={r.kind === 'dir' ? '폴더 이름' : '파일 이름'}
                onkeydown={(e) => { if (e.isComposing) return; if (e.key === 'Enter') { e.preventDefault(); void commit(); } else if (e.key === 'Escape') { e.preventDefault(); editing = null; } }}
                onblur={() => { if (editing && !editing.busy) { editing.value.trim() ? void commit() : (editing = null); } }} />
            {/if}
          </span>
        </div>
      {/if}
    {/each}
    {#if root?.truncated}<p class="note">항목이 많아 일부만 표시합니다.</p>{/if}
  </div>
  <p class="hint">파일을 가운데로 끌어다 놓거나 눌러서 편집합니다. 입력창으로 끌면 경로가 들어갑니다.</p>
</div>

<style>
  .tab { display: grid; gap: 8px; min-width: 0; align-content: start; }
  .bar { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
  .root { font-size: 12.5px; font-weight: 650; color: var(--ink-2); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .acts { display: flex; flex: none; }
  .sm { width: 28px; height: 28px; }
  .tree { display: grid; align-content: start; min-width: 0; margin: 0 -6px; min-height: 55dvh; }
  .note, .err { margin: 2px 0; padding-left: calc(30px + var(--d, 0) * 14px); font-size: 12.5px; color: var(--ink-3); }
  .err { color: var(--bad); }
  .hint { margin: 4px 0 0; font-size: 12px; color: var(--ink-3); }
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
  .od { flex: none; margin-left: auto; width: 6px; height: 6px; border-radius: 50%; background: var(--accent); }
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
