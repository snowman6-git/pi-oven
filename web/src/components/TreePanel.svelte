<script lang="ts">
  import { api, type SessionTree } from '../lib/api';
  import { app } from '../lib/app.svelte';
  import { showcase } from '../lib/showcase.svelte';
  import { branchCount, buildTurns, type Turn } from '../lib/tree';
  import { relativeTime } from '../lib/util';
  import Icon from './Icon.svelte';

  let { target }: { target: { path: string; cwd: string; title: string } } = $props();

  let dlg: HTMLDialogElement;
  let data = $state<SessionTree | null>(null);
  let error = $state<string | null>(null);
  let openId = $state<string | null>(null);
  let busy = $state(false);

  const tree = $derived(data ? buildTurns(data.entries, data.leafId, data.labels) : null);
  const branches = $derived(tree ? branchCount(tree) : 0);

  $effect(() => {
    dlg.showModal();
    let alive = true;
    api.sessionTree(target.path).then(
      (t) => {
        if (!alive) return;
        data = t;
        // Start on the turn the session stands in, which is where people usually want to look.
        const cur = buildTurns(t.entries, t.leafId, t.labels).rows.find((r) => r.turn.current);
        openId = cur?.turn.id ?? null;
      },
      (e) => alive && (error = (e as Error).message),
    );
    return () => { alive = false; };
  });

  // Bring the opened row into view once the list is there.
  $effect(() => {
    if (tree && openId) queueMicrotask(() => dlg.querySelector('[data-open="true"]')?.scrollIntoView({ block: 'center' }));
  });

  const clip = (s: string, n = 90) => (s.length > n ? s.slice(0, n) + '…' : s);
  const title = (t: Turn) => clip((t.user?.text || t.reply || '(시작)').replace(/\s+/g, ' '));

  async function branch(t: Turn, mode: 'here' | 'rewrite') {
    if (busy) return;
    busy = true;
    try {
      const b = mode === 'here'
        ? await api.branchSession(target.path, t.tip.id)
        : await api.branchSession(target.path, t.user?.parentId ?? null, t.user!.id);
      dlg.close();
      await app.openBranch(b);
    } catch (e) {
      error = (e as Error).message;
    } finally {
      busy = false;
    }
  }
</script>

<dialog bind:this={dlg} onclose={() => (app.treeTarget = null)} aria-labelledby="tree-title">
  <div class="box">
    <header>
      <div class="hd">
        <h2 id="tree-title">세션 트리</h2>
        <p class="sub" data-priv="titles">{target.title}</p>
      </div>
      <button class="btn quiet" type="button" onclick={() => dlg.close()}>닫기</button>
    </header>

    {#if error}<p class="err" role="alert">{error}</p>{/if}
    {#if !data && !error}<p class="empty">불러오는 중…</p>{/if}

    {#if tree}
      {#if tree.rows.length === 0}
        <p class="empty">아직 대화가 없습니다.</p>
      {:else}
        <p class="note">
          {#if branches > 1}분기가 {branches - 1}번 있습니다. 진한 점이 지금 이어지는 흐름이에요.
          {:else}분기 없이 한 줄로 이어진 대화입니다. 메시지를 골라 그 시점에서 새 흐름을 시작할 수 있어요.{/if}
        </p>
        <ul>
          {#each tree.rows as { turn: t, depth } (t.id)}
            {@const open = openId === t.id}
            <li style="--depth:{depth}" class:path={t.onPath} class:open data-open={open}>
              <button class="row" type="button" aria-expanded={open} onclick={() => (openId = open ? null : t.id)}>
                <span class="dot" class:on={t.onPath} aria-hidden="true"></span>
                <span class="main">
                  <span class="name" data-priv="chat">{showcase.text(title(t))}</span>
                  <span class="meta">
                    {#if t.current}<em>현재</em>{/if}
                    {#if t.label}<em class="label">{t.label}</em>{/if}
                    {#if t.toolCalls}도구 {t.toolCalls}회{/if}
                    {#if t.errors}<span class="bad">오류 {t.errors}</span>{/if}
                    {#if t.user?.ts}{relativeTime(Date.parse(t.user.ts))}{/if}
                  </span>
                </span>
              </button>
              {#if open}
                <div class="detail">
                  {#if t.user}<p class="q" data-priv="chat">{showcase.text(t.user.text)}</p>{/if}
                  {#if t.reply}<p class="a" data-priv="chat">{showcase.text(clip(t.reply, 400))}</p>{/if}
                  <div class="acts">
                    {#if t.user}
                      <button class="btn primary" type="button" disabled={busy} onclick={() => branch(t, 'rewrite')}>이 메시지부터 다시 쓰기</button>
                    {/if}
                    {#if !t.current}
                      <button class="btn" type="button" disabled={busy} onclick={() => branch(t, 'here')}>여기까지로 새 세션 열기</button>
                    {/if}
                  </div>
                </div>
              {/if}
            </li>
          {/each}
        </ul>
        <p class="foot"><Icon name="fork" size={14} /> 원본 세션은 그대로 두고, 고른 지점까지의 대화만 새 세션 파일로 복사해 엽니다.</p>
      {/if}
    {/if}
  </div>
</dialog>

<style>
  dialog {
    border: 1px solid var(--rule);
    border-radius: 14px;
    background: var(--raised);
    color: var(--ink);
    padding: 0;
    width: min(680px, calc(100vw - 24px));
    max-height: min(780px, calc(100dvh - 24px));
    box-shadow: var(--shadow);
  }
  dialog::backdrop { background: var(--scrim); }
  .box { padding: 0 20px 20px; display: grid; grid-template-columns: minmax(0, 1fr); gap: 12px; }
  header {
    display: flex; justify-content: space-between; align-items: center; gap: 12px;
    position: sticky; top: 0; padding: 16px 0 6px; background: var(--raised); z-index: 1;
  }
  .hd { min-width: 0; }
  h2 { margin: 0; font-size: 20px; letter-spacing: -0.02em; }
  .sub { margin: 2px 0 0; color: var(--ink-3); font-size: 13px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .empty, .note, .foot { margin: 0; color: var(--ink-2); font-size: 13.5px; }
  .foot { display: flex; gap: 6px; align-items: flex-start; color: var(--ink-3); font-size: 12.5px; }
  .foot :global(svg) { flex: none; margin-top: 2px; }
  .err { margin: 0; color: var(--bad); font-size: 13.5px; overflow-wrap: anywhere; }
  ul { list-style: none; margin: 0; padding: 0; display: grid; gap: 2px; }
  li {
    margin-left: calc(var(--depth) * 18px);
    border-radius: 10px;
    border: 1px solid transparent;
    min-width: 0;
  }
  li[style*="--depth:0"] { margin-left: 0; }
  li.open { background: var(--paper); border-color: var(--rule); }
  .row {
    width: 100%; display: grid; grid-template-columns: 12px minmax(0, 1fr); gap: 12px;
    align-items: start; padding: 9px 10px; text-align: left;
  }
  .dot {
    width: 10px; height: 10px; margin-top: 5px; border-radius: 50%;
    border: 1.5px solid var(--ink-3); background: transparent;
  }
  .dot.on { background: var(--heat); border-color: var(--heat); }
  .main { display: grid; gap: 1px; min-width: 0; }
  .name { overflow-wrap: anywhere; }
  li:not(.path) .name { color: var(--ink-2); }
  .meta { display: flex; flex-wrap: wrap; gap: 4px 10px; color: var(--ink-3); font-size: 12.5px; }
  .meta:empty { display: none; }
  em {
    font-style: normal; font-size: 11.5px; font-weight: 560; padding: 0 7px; border-radius: 999px;
    background: var(--accent-wash); color: var(--accent);
  }
  em.label { background: var(--code-bg); color: var(--ink-2); }
  .bad { color: var(--bad); }
  .detail { padding: 0 12px 12px 34px; display: grid; gap: 8px; }
  .q, .a { margin: 0; white-space: pre-wrap; overflow-wrap: anywhere; max-height: 28dvh; overflow: auto; }
  .q { font-weight: 560; }
  .a { color: var(--ink-2); font-size: 13.5px; }
  .acts { display: flex; flex-wrap: wrap; gap: 8px; }
  @media (max-width: 720px) {
    dialog { width: 100%; max-width: none; height: 100dvh; max-height: none; margin: 0; border: 0; border-radius: 0; box-shadow: none; }
    .box { padding: 0 16px calc(20px + env(safe-area-inset-bottom)); }
    header { padding-top: calc(14px + env(safe-area-inset-top)); }
    li { margin-left: calc(var(--depth) * 12px); }
    .detail { padding-left: 12px; }
  }
</style>
