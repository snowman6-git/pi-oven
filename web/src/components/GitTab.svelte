<script lang="ts">
  import { api, type GitFile, type GitOverview } from '../lib/api';
  import { app } from '../lib/app.svelte';
  import { showcase } from '../lib/showcase.svelte';
  import type { Chat } from '../lib/chat.svelte';
  import { relativeTime } from '../lib/util';
  import FileIcon from './FileIcon.svelte';
  import Icon from './Icon.svelte';
  import UnifiedDiff from './UnifiedDiff.svelte';

  let { chat }: { chat: Chat } = $props();

  let git = $state<GitOverview | null>(null);
  let error = $state<string | null>(null);
  let loading = $state(false);
  let busy = $state(false);
  let message = $state('');
  let problem = $state<string | null>(null);
  let sel = $state<string | null>(null);
  let diff = $state<{ path: string; text?: string; error?: string } | null>(null);
  let showIgnored = $state(false);

  const id = $derived(chat.runtime.id);
  const changes = $derived(git?.files.filter((f) => f.status !== '!') ?? []);
  const ignored = $derived(git?.files.filter((f) => f.status === '!') ?? []);
  const staged = $derived(changes.filter((f) => f.staged));
  const allStaged = $derived(changes.length > 0 && changes.every((f) => f.staged && !f.unstaged));
  const statusName: Record<string, string> = { M: '수정', A: '추가', D: '삭제', R: '이름 변경', '?': '새 파일', U: '충돌' };

  async function load() {
    if (loading) return;
    loading = true;
    try {
      git = await api.git(id);
      error = null;
    } catch (e) {
      error = (e as Error).message;
    } finally {
      loading = false;
    }
    if (sel) void loadDiff(sel);
  }

  async function loadDiff(path: string) {
    try { diff = { path, text: await api.gitDiff(id, path) }; } catch (e) { diff = { path, error: (e as Error).message }; }
  }

  function pick(f: GitFile) {
    sel = sel === f.path ? null : f.path;
    diff = null;
    if (sel) void loadDiff(sel);
  }

  async function stage(paths: string[], on: boolean) {
    if (busy || paths.length === 0) return;
    busy = true;
    problem = null;
    try {
      await api.gitStage(id, paths, on);
    } catch (e) {
      problem = (e as Error).message;
    } finally {
      busy = false;
    }
    await load();
  }

  async function commit() {
    const msg = message.trim();
    if (busy || !msg || staged.length === 0) return;
    busy = true;
    problem = null;
    try {
      const r = await api.gitCommit(id, msg);
      message = '';
      sel = null;
      diff = null;
      app.flash(`커밋했습니다 (${r.hash})`);
    } catch (e) {
      problem = (e as Error).message;
    } finally {
      busy = false;
    }
    await load();
  }

  $effect(() => { void load(); });
  // pi works in this folder: keep up while it does, and look once more when it stops.
  $effect(() => {
    if (!chat.running) return;
    const t = setInterval(load, 4000);
    return () => clearInterval(t);
  });
  $effect(() => { if (!chat.running) void load(); });

  const dir = (p: string) => (p.includes('/') ? p.slice(0, p.lastIndexOf('/', p.length - 2) + 1) : '');
  const name = (p: string) => p.slice(dir(p).length);
  const indeterminate = (node: HTMLInputElement, on: boolean) => {
    node.indeterminate = on;
    return { update: (v: boolean) => (node.indeterminate = v) };
  };
</script>

<div class="tab">
  {#if error}<p class="err" role="alert">{error}</p>{/if}

  {#if git && !git.repo}
    <p class="note">이 폴더는 Git 저장소가 아닙니다.</p>
  {:else if git}
    <div class="bar">
      <p class="branch">{#if git.branch}<code class="mono">{git.branch}</code>{/if}</p>
      <div class="acts">
        <button class="icon-btn sm" type="button" title="새로 고침" aria-label="새로 고침" disabled={loading} onclick={load}><Icon name="refresh" size={15} /></button>
      </div>
    </div>

    {#if changes.length === 0}
      <p class="note">바뀐 파일이 없습니다.</p>
    {:else}
      <label class="all"><input type="checkbox" checked={allStaged} use:indeterminate={staged.length > 0 && !allStaged} disabled={busy}
        onchange={() => stage(changes.map((f) => f.path), !allStaged)} /> 전체 선택 <span>{staged.length}/{changes.length}</span></label>
      <ul>
        {#each changes as f (f.path)}
          {@const open = sel === f.path}
          <li class:open>
            <div class="row">
              <input type="checkbox" aria-label="{f.path} 스테이지" checked={f.staged && !f.unstaged} use:indeterminate={f.staged && f.unstaged} disabled={busy}
                onchange={() => stage([f.path], !(f.staged && !f.unstaged))} />
              <button class="p" type="button" aria-expanded={open} onclick={() => pick(f)} title={f.path}>
                <span class="path mono">{#if !f.path.endsWith('/')}<span class="fic"><FileIcon name={name(f.path)} size={14} /></span>{/if}<span class="nm" data-priv="files">{name(f.path)}</span>{#if dir(f.path)} <span class="d" data-priv="files">{dir(f.path)}</span>{/if}</span>
                <span class="meta">
                  <span class="st st-{f.status === '?' ? 'n' : f.status}">{statusName[f.status] ?? f.status}</span>
                  {#if f.added != null}<b class="add">+{f.added}</b> <b class="del">−{f.removed ?? 0}</b>{/if}
                </span>
              </button>
            </div>
            {#if open}
              <div class="body">
                {#if diff && diff.path === f.path}
                  {#if diff.error}<p class="err">{diff.error}</p>
                  {:else if (diff.text ?? '').trim() === ''}<p class="note">보여줄 내용이 없습니다.</p>
                  {:else}<UnifiedDiff text={diff.text ?? ''} />{/if}
                {:else}<p class="note">불러오는 중…</p>{/if}
              </div>
            {/if}
          </li>
        {/each}
      </ul>
      {#if git.truncated}<p class="note">파일이 많아 일부만 표시합니다.</p>{/if}
    {/if}

    <form class="commit" onsubmit={(e) => { e.preventDefault(); void commit(); }}>
      <textarea class="field" rows="3" placeholder="커밋 메시지" aria-label="커밋 메시지" bind:value={message}
        onkeydown={(e) => { if (e.key === 'Enter' && (e.ctrlKey || e.metaKey) && !e.isComposing) { e.preventDefault(); void commit(); } }}></textarea>
      <button class="btn primary" type="submit" disabled={busy || !message.trim() || staged.length === 0}
        title={staged.length === 0 ? '커밋할 파일을 먼저 체크하세요' : 'Ctrl+Enter'}>
        {staged.length ? `${staged.length}개 파일 커밋` : '커밋'}
      </button>
      {#if problem}<pre class="err mono" role="alert">{problem}</pre>{/if}
    </form>

    {#if ignored.length}
      <button class="fold" type="button" aria-expanded={showIgnored} onclick={() => (showIgnored = !showIgnored)}>
        <span class:up={showIgnored}><Icon name="right" size={13} /></span> 무시됨 <span class="cnt">{ignored.length}</span>
      </button>
      {#if showIgnored}
        <ul class="ign">
          {#each ignored as f (f.path)}<li class="mono" title="{f.path} (.gitignore)">{f.path}</li>{/each}
        </ul>
      {/if}
    {/if}

    {#if git.commits.length}
      <h4>최근 커밋</h4>
      <ul class="log">
        {#each git.commits as c (c.hash)}
          <li><code class="mono">{c.hash}</code><span class="subj" title={c.subject}>{c.subject}</span><small>{relativeTime(c.when * 1000)}</small></li>
        {/each}
      </ul>
    {/if}
  {:else if !error}
    <p class="note">불러오는 중…</p>
  {/if}
</div>

<style>
  .tab { display: grid; gap: 10px; min-width: 0; }
  p { margin: 0; }
  .note { color: var(--ink-3); font-size: 13px; }
  .err { color: var(--bad); font-size: 13px; overflow-wrap: anywhere; margin: 0; }
  pre.err { white-space: pre-wrap; font-size: 12px; max-height: 30dvh; overflow: auto; }
  .bar { display: flex; justify-content: space-between; align-items: center; }
  .branch code { font-size: 12.5px; padding: 1px 8px; border-radius: 999px; background: var(--accent-wash); color: var(--accent); }
  .sm { width: 28px; height: 28px; }
  .all { display: flex; align-items: center; gap: 8px; font-size: 12.5px; color: var(--ink-2); }
  .all span { color: var(--ink-3); }
  input[type='checkbox'] { accent-color: var(--accent); width: 15px; height: 15px; flex: none; margin: 0; }
  ul { list-style: none; margin: 0; padding: 0; display: grid; gap: 3px; }
  li { border-radius: 8px; min-width: 0; }
  li.open { background: var(--paper); box-shadow: inset 0 0 0 1px var(--rule); }
  .row { display: flex; align-items: flex-start; gap: 8px; padding: 6px 8px; }
  .row input { margin-top: 3px; }
  .p { flex: 1; min-width: 0; display: grid; text-align: left; gap: 1px; }
  .path { font-size: 12.5px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .d { color: var(--ink-3); margin-left: 7px; }
  .fic { display: inline-flex; vertical-align: -3px; margin-right: 7px; }
  .meta { display: flex; gap: 8px; font-size: 11.5px; color: var(--ink-3); }
  .st-M { color: var(--accent); }
  .st-n, .st-A { color: var(--ok); }
  .st-D, .st-U { color: var(--bad); }
  .add { color: var(--ok); font-weight: 600; }
  .del { color: var(--bad); font-weight: 600; }
  .body { padding: 0 8px 8px; min-width: 0; }
  .body :global(.diff) { max-height: 40dvh; font-size: 12px; }
  .commit { display: grid; gap: 8px; }
  textarea { resize: vertical; min-height: 60px; font: inherit; font-size: 13.5px; }
  .fold { display: flex; align-items: center; gap: 4px; font-size: 12.5px; color: var(--ink-3); }
  .fold span:first-child { display: inline-grid; transition: rotate 0.12s; }
  .fold .up { rotate: 90deg; }
  .cnt { color: var(--ink-3); }
  .ign li { font-size: 12px; color: var(--ink-3); opacity: 0.7; padding: 1px 8px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  h4 { margin: 4px 0 0; font-size: 12.5px; color: var(--ink-2); }
  .log li { display: grid; grid-template-columns: auto minmax(0, 1fr) auto; gap: 8px; align-items: baseline; font-size: 12.5px; padding: 2px 4px; }
  .log code { color: var(--accent); font-size: 11.5px; }
  .subj { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .log small { color: var(--ink-3); font-size: 11.5px; }
</style>
