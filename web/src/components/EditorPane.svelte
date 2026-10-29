<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { app } from '../lib/app.svelte';
  import { showcase } from '../lib/showcase.svelte';
  import type { Chat } from '../lib/chat.svelte';
  import type { Doc } from '../lib/editor.svelte';
  import { merge3 } from '../lib/merge';
  import Icon from './Icon.svelte';

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
  const ready = $derived(!!doc && !doc.loading && !doc.error);

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
      void cm!.languageFor(d.path).then((ext) => { if (shown === d) view!.dispatch({ effects: cm!.language.reconfigure(ext) }); });
      view!.dispatch({ effects: cm!.wrapping.reconfigure(wrap ? cm!.EditorView.lineWrapping : []) });
      const h = view!.state.selection.main.head;
      const ln = view!.state.doc.lineAt(h);
      pos = { line: ln.number, col: h - ln.from + 1 };
      publishSelection(d, view!.state);
      requestAnimationFrame(() => {
        view!.requestMeasure();
        if (matchMedia('(pointer: fine)').matches && editors.active === d.path) view!.focus();
      });
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

  function onKey(e: KeyboardEvent) {
    // Ctrl/Cmd+S saves the file in view even when the cursor is not in the editor
    if ((e.ctrlKey || e.metaKey) && !e.altKey && e.key.toLowerCase() === 's' && doc && visible) {
      e.preventDefault();
      void editors.save(doc.path);
    }
  }
</script>

<svelte:window onkeydown={onKey} />

<section class="pane" class:hidden={!visible} aria-hidden={!visible} aria-label="파일 편집기">
  {#if doc?.conflict === 'changed'}
    <div class="banner warn" role="alert">
      <span>디스크의 파일이 바뀌었습니다. pi나 다른 프로그램이 고쳤을 수 있어요.</span>
      <span class="acts">
        <button class="btn" type="button" onclick={() => editors.reload(doc.path)}>디스크 내용 불러오기</button>
        <button class="btn" type="button" onclick={() => editors.save(doc.path, true)}>내 내용으로 덮어쓰기</button>
      </span>
    </div>
  {:else if doc?.conflict === 'deleted'}
    <div class="banner warn" role="alert">
      <span>이 파일이 디스크에서 삭제되었습니다. 저장하면 다시 만듭니다.</span>
      <span class="acts"><button class="btn" type="button" onclick={() => editors.save(doc.path, true)}>다시 만들어 저장</button></span>
    </div>
  {/if}

  <div class="body">
    <div class="cm" bind:this={host} class:off={!ready}></div>
    {#if doc && !ready}
      <div class="msg">
        {#if doc.loading}<p>불러오는 중…</p>
        {:else}
          <p>{doc.error}</p>
          <button class="btn" type="button" onclick={() => editors.close(doc.path, false)}>탭 닫기</button>
        {/if}
      </div>
    {/if}
  </div>

  {#if doc && ready}
    <footer>
      <span class="path mono" title={showcase.tip('files', doc.path)} data-priv="files">{doc.path}</span>
      <span class="grow"></span>
      <span class="stat">줄 {pos.line}, 열 {pos.col}</span>
      <span class="stat">{doc.eol === 'crlf' ? 'CRLF' : 'LF'}</span>
      <button class="chip" type="button" aria-pressed={wrap} onclick={toggleWrap} title="긴 줄 바꿈">줄바꿈</button>
      <button class="btn save" type="button" disabled={!doc.dirty || doc.saving} onclick={() => editors.save(doc.path)} title="Ctrl+S">
        {doc.saving ? '저장 중…' : doc.dirty ? '저장' : '저장됨'}
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
