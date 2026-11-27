<script lang="ts">
  import { S } from '../lib/strings';
  import { t } from '../lib/i18n.svelte';
  import { onDestroy, onMount, untrack } from 'svelte';
  import { app } from '../lib/app.svelte';
  import { showcase } from '../lib/showcase.svelte';
  import type { Chat } from '../lib/chat.svelte';
  import type { Doc } from '../lib/editor.svelte';
  import { merge3 } from '../lib/merge';
  import { api } from '../lib/api';
  import { compactNumber } from '../lib/util';
  import Icon from './Icon.svelte';
  import Markdown from './Markdown.svelte';

  let { chat, splitOn = false }: { chat: Chat; splitOn?: boolean } = $props();
  const editors = $derived(chat.editors);

  // The editor is loaded on first use; the conversation view does not carry it.
  type CM = typeof import('../lib/cm');
  let cm = $state.raw<CM | null>(null);
  let view: import('@codemirror/view').EditorView | null = null;
  let host = $state<HTMLElement>();

  // What each file in a tab keeps while another is in view: its text, cursor, undo history. Keyed by the
  // file's tab (not its path), so renaming it does not lose what was typed.
  type Entry = { state: import('@codemirror/state').EditorState; saved: import('@codemirror/state').Text; version: number };
  const entries = new Map<Doc, Entry>();
  let shown: Doc | null = null;

  let wrap = $state(readWrap());
  let pos = $state({ line: 1, col: 1 });

  function readWrap() { try { return localStorage.getItem('pi-oven.editor-wrap') !== 'off'; } catch { return true; } }

  const doc = $derived.by(() => {
    const p = splitOn ? (editors.active ?? editors.last) : editors.active;
    return p ? editors.get(p) : undefined;
  });
  const visible = $derived(!!doc);
  const ready = $derived(!!doc && !doc.loading && !doc.error && !doc.image);
  const picture = $derived(!!doc && !doc.loading && !doc.error && !!doc.image);
  let dims = $state<{ w: number; h: number } | null>(null);
  let broken = $state(false);
  /** The picture on screen, and which file it is of: a new version replaces it only once it has arrived (no blink). */
  let img = $state<{ url: string; path: string } | null>(null);
  function showImg(next: { url: string; path: string } | null) {
    const old = untrack(() => img);
    img = next;
    if (old) URL.revokeObjectURL(old.url);
  }
  $effect(() => {
    doc?.path;
    doc?.version;
    dims = null;
    broken = false;
  });
  // the picture is fetched with the login header and shown from a blob URL, which is released when it changes or goes
  $effect(() => {
    if (!doc || !picture) return untrack(() => showImg(null));
    const ctl = new AbortController();
    const path = doc.path;
    api.rawBlobUrl(chat.runtime.id, path, doc.version, ctl.signal).then(
      (u) => { if (ctl.signal.aborted) URL.revokeObjectURL(u); else showImg({ url: u, path }); },
      () => { if (!ctl.signal.aborted) broken = true; },
    );
    return () => ctl.abort();
  });
  onDestroy(() => showImg(null));

  onMount(() => {
    let alive = true;
    void import('../lib/cm').then((m) => {
      if (!alive || !host) return;
      cm = m;
      view = m.makeView(host, m.createState('', '  ', { onChange() {}, onSave() {} }, wrap));
    });
    editors.textOf = (path) => {
      const d = editors.get(path);
      if (!d) return null;
      if (d === shown && view) return view.state.doc.toString();
      return entries.get(d)?.state.doc.toString() ?? null;
    };
    editors.onSaved = (path, text) => {
      const d = editors.get(path);
      const e = d && entries.get(d);
      if (!d || !e || !cm) return;
      e.saved = cm.Text.of(text.split('\n'));
      const now = d === shown && view ? view.state.doc : e.state.doc;
      editors.setDirty(path, !now.eq(e.saved));
    };
    editors.applyRemote = (path, f) => {
      const d = editors.get(path);
      const e = d && entries.get(d);
      if (!d || !e || !cm) return { result: 'none' };
      const live = d === shown && view;
      if (live && view!.composing) return { result: 'later' }; // not under someone typing a syllable
      const mine = live ? view!.state.doc.toString() : e.state.doc.toString();
      const m = merge3(d.base, f.content, mine);
      if (!m.ok) return { result: 'conflict' };
      e.saved = cm.Text.of(f.content.split('\n')); // set first: the change below is checked against it
      if (m.changes.length) {
        if (live) cm.applyRemote(view!, m.changes);
        else e.state = cm.applyRemoteToState(e.state, m.changes);
      }
      const now = live ? view!.state.doc : e.state.doc;
      editors.setDirty(path, !now.eq(e.saved));
      return { result: 'applied', lines: m.changes.reduce((n, c) => n + Math.max(1, (c.insert.match(/\n/g)?.length ?? 0) + (c.insert && !c.insert.endsWith('\n') ? 1 : 0)), 0) };
    };
    return () => {
      alive = false;
      editors.applyRemote = null;
      view?.destroy();
      view = null;
      editors.textOf = null;
      editors.onSaved = null;
    };
  });

  /** What is selected, for sending to pi with a message. */
  function publishSelection(d: Doc, st: import('@codemirror/state').EditorState) {
    const { from, to } = st.selection.main;
    const doc = st.doc;
    // a selection that ends at the very start of a line does not include that line
    const lastPos = to > from && doc.lineAt(to).from === to ? to - 1 : to;
    editors.selection = { path: d.path, from: doc.lineAt(from).number, to: doc.lineAt(lastPos).number, text: from === to ? '' : st.sliceDoc(from, Math.min(to, from + 60_000)) };
  }

  function stateFor(d: Doc): Entry {
    let e = entries.get(d);
    if (e && e.version === d.version) return e;
    const c = cm!;
    const state = c.createState(d.base, c.indentFor(d.base), {
      onChange: (u) => {
        if (u.docChanged) editors.setDirty(d.path, !u.state.doc.eq(entries.get(d)!.saved));
        if ((u.docChanged || u.selectionSet) && d === shown) {
          const h = u.state.selection.main.head;
          const ln = u.state.doc.lineAt(h);
          pos = { line: ln.number, col: h - ln.from + 1 };
          publishSelection(d, u.state);
        }
      },
      onSave: () => void editors.save(d.path),
    }, wrap);
    e = { state, saved: state.doc, version: d.version };
    entries.set(d, e);
    return e;
  }

  // Show the file in view: park the one that was there, put this one's own state in.
  $effect(() => {
    const d = doc;
    const ok = ready;
    void d?.version;
    if (!cm || !view || !d || !ok) return;
    untrack(() => {
      if (shown && shown !== d && entries.has(shown)) entries.get(shown)!.state = view!.state;
      const e = stateFor(d);
      view!.setState(e.state);
      shown = d;
      void cm!.languageFor(d.path).then((ext) => { if (view && shown === d) view.dispatch({ effects: cm!.language.reconfigure(ext) }); });
      view!.dispatch({ effects: cm!.wrapping.reconfigure(wrap ? cm!.EditorView.lineWrapping : []) });
      const h = view!.state.selection.main.head;
      const ln = view!.state.doc.lineAt(h);
      pos = { line: ln.number, col: h - ln.from + 1 };
      publishSelection(d, view!.state);
      const raf = requestAnimationFrame(() => {
        if (!view) return;
        view.requestMeasure();
        if (matchMedia('(pointer: fine)').matches && editors.active === d.path) view.focus();
      });
      return () => cancelAnimationFrame(raf);
    });
  });

  // The composer offers to send what is in view with a message.
  $effect(() => {
    const p = ready && visible ? doc!.path : null;
    editors.inView = p;
    return () => { if (editors.inView === p) editors.inView = null; };
  });

  // A tab that has been closed takes its parked state with it.
  $effect(() => {
    const live = new Set(editors.docs);
    for (const d of [...entries.keys()]) if (!live.has(d)) entries.delete(d);
    if (shown && !live.has(shown)) shown = null;
  });

  function toggleWrap() {
    wrap = !wrap;
    try { localStorage.setItem('pi-oven.editor-wrap', wrap ? 'on' : 'off'); } catch { /* private mode */ }
    if (cm && view) view.dispatch({ effects: cm.wrapping.reconfigure(wrap ? cm.EditorView.lineWrapping : []) });
  }

  // Markdown can be read rendered, by the same component as replies: the same sanitiser (DOMPurify: no scripts, no
  // event attributes, no javascript: links; pictures from elsewhere become links), so a file cannot run anything here.
  const isMd = $derived(!!doc && /\.(md|markdown|mdx)$/i.test(doc.path));
  const previewing = $derived(!!doc && isMd && editors.previewing.includes(doc.path));
  let rendered = $state('');
  let renderTimer: ReturnType<typeof setTimeout> | undefined;
  function renderNow() {
    rendered = doc && view ? view.state.doc.toString() : '';
  }
  $effect(() => {
    if (!previewing) return;
    doc?.version;
    untrack(renderNow);
  });
  function togglePreview() {
    if (!doc) return;
    const p = doc.path;
    editors.previewing = editors.previewing.includes(p) ? editors.previewing.filter((x) => x !== p) : [...editors.previewing, p];
    if (editors.previewing.includes(p)) {
      clearTimeout(renderTimer);
      renderNow();
    }
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape' && editors.full && visible && !e.defaultPrevented) {
      editors.full = false;
      return;
    }
    // Ctrl/Cmd+S saves the file in view even when the cursor is not in the editor
    if ((e.ctrlKey || e.metaKey) && !e.altKey && e.key.toLowerCase() === 's' && doc && visible) {
      e.preventDefault();
      void editors.save(doc.path);
    }
  }
</script>

<svelte:window onkeydown={onKey} />

{#snippet fullBtn()}
  <button class="icon-btn full" type="button" aria-pressed={editors.full} onclick={() => (editors.full = !editors.full)}
    title={editors.full ? t('editor.fullOff') : t('editor.fullOn')} aria-label={editors.full ? t('editor.fullOff') : t('editor.fullOn')}>
    <Icon name={editors.full ? 'collapse' : 'expand'} size={16} />
  </button>
{/snippet}

<section class="pane" class:hidden={!visible} aria-hidden={!visible} aria-label={t('editor.label')}>
  {#if doc?.conflict === 'changed'}
    <div class="banner warn" role="alert">
      <span>{t('editor.changedOnDisk')}</span>
      <span class="acts">
        <button class="btn" type="button" onclick={() => editors.reload(doc.path)}>{t('editor.loadDisk')}</button>
        <button class="btn" type="button" onclick={() => editors.save(doc.path, true)}>{t('editor.overwrite')}</button>
      </span>
    </div>
  {:else if doc?.conflict === 'deleted'}
    <div class="banner warn" role="alert">
      <span>{t('editor.deleted')}</span>
      <span class="acts"><button class="btn" type="button" onclick={() => editors.save(doc.path, true)}>{t('editor.recreate')}</button></span>
    </div>
  {/if}

  <div class="body">
    <div class="cm" bind:this={host} class:off={!ready || previewing}></div>
    {#if ready && previewing}
      <div class="md-view" data-priv="files"><Markdown source={rendered} /></div>
    {/if}
    {#if doc && picture}
      <div class="pic" data-priv="files">
        {#if broken}
          <p class="msg-inline">{t('editor.imageBroken')}</p>
        {:else if img && img.path === doc.path}
          <img src={img.url} alt={doc.name}
            onload={(e) => { const i = e.currentTarget as HTMLImageElement; dims = { w: i.naturalWidth, h: i.naturalHeight }; }}
            onerror={() => (broken = true)} />
        {/if}
      </div>
    {:else if doc && !ready}
      <div class="msg">
        {#if doc.loading}<p>{S.loading}</p>
        {:else}
          <p>{doc.error}</p>
          <button class="btn" type="button" onclick={() => editors.close(doc.path, false)}>{t('editor.closeTab')}</button>
        {/if}
      </div>
    {/if}
  </div>

  {#if doc && picture}
    <footer>
      <span class="path mono" title={showcase.tip('files', doc.path)} data-priv="files">{doc.path}</span>
      <span class="grow"></span>
      {#if dims}<span class="stat">{dims.w} × {dims.h}</span>{/if}
      {#if doc.size != null}<span class="stat">{compactNumber(doc.size)}B</span>{/if}
      {@render fullBtn()}
    </footer>
  {/if}

  {#if doc && ready}
    <footer>
      <span class="path mono" title={showcase.tip('files', doc.path)} data-priv="files">{doc.path}</span>
      <span class="grow"></span>
      <span class="stat">{t('editor.pos', { line: pos.line, col: pos.col })}</span>
      <span class="stat">{doc.eol === 'crlf' ? 'CRLF' : 'LF'}</span>
      {#if isMd}<button class="chip" type="button" aria-pressed={previewing} onclick={togglePreview}>{t('editor.preview')}</button>{/if}
      <button class="chip" type="button" aria-pressed={wrap} onclick={toggleWrap} title={t('editor.wrapHint')}>{t('editor.wrap')}</button>
      {@render fullBtn()}
      <button class="btn save" type="button" disabled={!doc.dirty || doc.saving} onclick={() => editors.save(doc.path)} title="Ctrl+S">
        {doc.saving ? t('editor.saving') : doc.dirty ? t('editor.save') : t('editor.saved')}
      </button>
    </footer>
  {/if}
</section>

<style>
  .pane {
    position: relative;
    flex: 1;
    display: flex;
    flex-direction: column;
    min-height: 0;
    background: var(--paper);
  }
  .pane.hidden {
    visibility: hidden;
    pointer-events: none;
  }
  .body {
    position: relative;
    flex: 1;
    min-height: 0;
  }
  .cm {
    position: absolute;
    inset: 0;
  }
  .cm.off {
    visibility: hidden;
  }
  .cm :global(.cm-editor) {
    height: 100%;
  }
  .pic {
    position: absolute;
    inset: 0;
    overflow: auto;
    display: grid;
    place-items: center;
    padding: 16px;
    /* a checkerboard, so transparent parts read as transparent */
    background: repeating-conic-gradient(var(--paper-sunk) 0% 25%, var(--paper) 0% 50%) 50% / 20px 20px;
  }
  .pic img {
    max-width: 100%;
    max-height: 100%;
    object-fit: contain;
    image-rendering: auto;
    box-shadow: 0 0 0 1px var(--rule);
  }
  .msg-inline {
    color: var(--ink-2);
  }
  .msg {
    position: absolute;
    inset: 0;
    display: grid;
    place-content: center;
    justify-items: center;
    gap: 12px;
    color: var(--ink-2);
    text-align: center;
    padding: 0 24px;
  }
  .msg p {
    margin: 0;
  }
  .banner {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 8px 16px;
    padding: 9px 16px;
    border-bottom: 1px solid var(--rule);
    background: var(--heat-wash);
    color: var(--ink);
    font-size: 13.5px;
  }
  .acts {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  footer {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 6px 12px 6px 16px;
    border-top: 1px solid var(--rule);
    background: var(--paper-sunk);
    font-size: 12px;
    color: var(--ink-3);
    min-height: 40px;
  }
  .path {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    direction: rtl;
    text-align: left;
  }
  .grow {
    flex: 1;
  }
  .stat {
    white-space: nowrap;
  }
  .md-view {
    position: absolute;
    inset: 0;
    overflow: auto;
    padding: 20px max(20px, calc((100% - 760px) / 2));
    font-size: 14.5px;
    line-height: 1.65;
    overflow-wrap: anywhere;
  }
  .full {
    width: 28px;
    height: 28px;
  }
  .chip {
    padding: 2px 9px;
    border-radius: 999px;
    border: 1px solid var(--rule);
    color: var(--ink-3);
  }
  .chip[aria-pressed='true'] {
    color: var(--ink);
    border-color: var(--ink-3);
  }
  .save {
    height: 28px;
    padding: 0 12px;
  }
  @media (max-width: 720px) {
    .path,
    .stat:nth-of-type(2) {
      display: none;
    }
  }
</style>
