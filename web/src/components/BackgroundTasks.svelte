<script lang="ts">
  import { api } from '../lib/api';
  import { app } from '../lib/app.svelte';
  import type { Chat } from '../lib/chat.svelte';
  import type { BgTask } from '../lib/protocol';
  import { compactNumber, duration } from '../lib/util';
  import Icon from './Icon.svelte';

  let { chat }: { chat: Chat } = $props();

  let dlg: HTMLDialogElement;
  let tasks = $state<BgTask[]>([]);
  let loaded = $state(false);
  let error = $state<string | null>(null);
  // svelte-ignore state_referenced_locally
  let openId = $state<string | null>(chat.bgFocus); // only the starting value: which task the click that opened this was on
  let out = $state<{ text: string; total: number }>({ text: '', total: 0 });
  let outBox = $state<HTMLElement>();
  let now = $state(Date.now());

  const label = { running: '실행 중', completed: '완료', failed: '실패', killed: '중단됨' } as const;
  const finishedCount = $derived(chat.bg.failed + chat.bg.stopped + chat.bg.done);

  async function refresh() {
    try {
      tasks = await api.tasks(chat.runtime.id);
      error = null;
    } catch (e) {
      error = (e as Error).message;
    } finally {
      loaded = true;
    }
    if (openId) {
      try {
        const next = await api.taskOutput(chat.runtime.id, openId);
        const pinned = !outBox || outBox.scrollHeight - outBox.scrollTop - outBox.clientHeight < 40;
        out = next;
        if (pinned) queueMicrotask(() => outBox && (outBox.scrollTop = outBox.scrollHeight));
      } catch { /* the next poll will retry */ }
    }
  }

  // Live while the dialog is open: poll, and tick the elapsed time every second.
  $effect(() => {
    dlg.showModal();
    void refresh();
    const poll = setInterval(refresh, 2000);
    const tick = setInterval(() => (now = Date.now()), 1000);
    return () => { clearInterval(poll); clearInterval(tick); };
  });

  // Status changes in the status line (a task ended) are worth an immediate refresh.
  $effect(() => {
    chat.bg.running; chat.bg.failed; chat.bg.done; chat.bg.stopped;
    if (dlg?.open) void refresh();
  });

  function toggle(t: BgTask) {
    openId = openId === t.id ? null : t.id;
    out = { text: '', total: 0 };
    if (openId) void refresh();
  }

  const elapsed = (t: BgTask) => duration((t.endTime ?? now) - t.startTime);
  const title = (t: BgTask) => t.name || t.description || t.command.split('\n')[0];
  const detail = (t: BgTask) => {
    const parts: string[] = [label[t.status]];
    if (t.status !== 'running' && t.exitCode != null) parts.push(`종료 코드 ${t.exitCode}`);
    parts.push(elapsed(t));
    if (t.bytesWritten) parts.push(`${compactNumber(t.bytesWritten)}B`);
    return parts.join(', ');
  };

  function clearSeen() {
    chat.conn.request({ type: 'prompt', message: '/bg-clear' }).catch(() => {});
  }
</script>

<dialog bind:this={dlg} onclose={() => ((chat.bgOpen = false), (chat.bgFocus = null))} aria-labelledby="bg-title">
  <div class="box">
    <header>
      <h2 id="bg-title">백그라운드 작업</h2>
      <div class="actions">
        {#if finishedCount > 0}
          <button class="btn quiet" type="button" onclick={clearSeen} title="확인한 완료·실패 표시를 지웁니다">완료 항목 지우기</button>
        {/if}
        <button class="btn quiet" type="button" onclick={() => dlg.close()}>닫기</button>
      </div>
    </header>

    {#if error}<p class="err" role="alert">{error}</p>{/if}

    {#if loaded && tasks.length === 0}
      <p class="empty">이 세션에서 시작한 백그라운드 작업이 없습니다. pi가 오래 걸리는 명령을 <code class="mono">bg_run</code>으로 시작하면 여기에 나타납니다.</p>
    {/if}

    <ul>
      {#each tasks as t (t.id)}
        <li class:open={openId === t.id}>
          <button class="row" type="button" aria-expanded={openId === t.id} onclick={() => toggle(t)}>
            <span class="dot {t.status}" role="img" aria-label={label[t.status]}></span>
            <span class="main">
              <span class="name">{title(t)}{#if t.isAgent}<em>에이전트</em>{/if}</span>
              <span class="cmd mono">{t.command.split('\n')[0]}</span>
              <span class="meta">{detail(t)}</span>
            </span>
            <span class="chev" class:up={openId === t.id}><Icon name="down" size={16} /></span>
          </button>

          {#if openId === t.id}
            <div class="out">
              {#if t.error}<p class="err">{t.error}</p>{/if}
              <pre class="mono" bind:this={outBox}>{out.text || (t.status === 'running' ? '아직 출력이 없습니다…' : '출력이 없습니다.')}</pre>
              <div class="foot">
                {#if out.total > out.text.length}<small>끝부분만 표시 ({compactNumber(out.total)}B 중)</small>{/if}
                <span class="grow"></span>
                <button class="btn" type="button" disabled={!out.text} onclick={() => app.copy(out.text)}>출력 복사</button>
              </div>
            </div>
          {/if}
        </li>
      {/each}
    </ul>
  </div>
</dialog>

<style>
  dialog {
    border: 1px solid var(--rule);
    border-radius: 14px;
    background: var(--raised);
    color: var(--ink);
    padding: 0;
    width: min(640px, calc(100vw - 24px));
    max-height: min(720px, calc(100dvh - 24px));
    box-shadow: var(--shadow);
  }
  dialog::backdrop {
    background: var(--scrim);
  }
  .box {
    padding: 0 20px 20px;
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 12px;
  }
  header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    position: sticky;
    top: 0;
    padding: 16px 0 6px;
    background: var(--raised);
    z-index: 1;
  }
  h2 {
    margin: 0;
    font-size: 20px;
    letter-spacing: -0.02em;
  }
  .actions {
    display: flex;
    gap: 2px;
  }
  .empty {
    margin: 8px 0 16px;
    color: var(--ink-2);
  }
  .err {
    margin: 0;
    color: var(--bad);
    font-size: 13.5px;
    overflow-wrap: anywhere;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 6px;
  }
  li {
    border: 1px solid var(--rule);
    border-radius: 10px;
    background: var(--paper);
    min-width: 0;
  }
  li.open {
    border-color: var(--ink-3);
  }
  .row {
    width: 100%;
    display: grid;
    grid-template-columns: 12px minmax(0, 1fr) auto;
    gap: 12px;
    align-items: start;
    padding: 11px 12px;
    text-align: left;
  }
  .dot {
    width: 9px;
    height: 9px;
    margin-top: 6px;
    border-radius: 2px;
    background: var(--ink-3);
  }
  .dot.running {
    background: var(--heat);
    animation: bg-ember 1.2s ease-in-out infinite;
  }
  .dot.completed {
    background: var(--ok);
  }
  .dot.failed {
    background: var(--bad);
  }
  @keyframes bg-ember {
    50% {
      box-shadow: 0 0 0 4px color-mix(in srgb, var(--heat) 28%, transparent);
    }
  }
  .main {
    display: grid;
    gap: 1px;
    min-width: 0;
  }
  .name {
    font-weight: 650;
    overflow-wrap: anywhere;
  }
  .name em {
    margin-left: 8px;
    font-style: normal;
    font-size: 11.5px;
    font-weight: 560;
    padding: 1px 7px;
    border-radius: 999px;
    background: var(--accent-wash);
    color: var(--accent);
  }
  .cmd {
    color: var(--ink-3);
    font-size: 12px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .meta {
    color: var(--ink-2);
    font-size: 12.5px;
  }
  .chev {
    margin-top: 2px;
    color: var(--ink-3);
    transition: transform 0.12s;
  }
  .chev.up {
    transform: rotate(180deg);
  }
  .out {
    padding: 0 12px 12px;
    display: grid;
    gap: 8px;
  }
  pre {
    margin: 0;
    padding: 10px 12px;
    background: var(--code-bg);
    border-radius: 8px;
    max-height: 42dvh;
    overflow: auto;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    font-size: 12.5px;
    line-height: 1.55;
  }
  .foot {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .grow {
    flex: 1;
  }
  small {
    color: var(--ink-3);
    font-size: 12.5px;
  }
  @media (max-width: 720px) {
    dialog {
      width: 100%;
      max-width: none;
      height: 100dvh;
      max-height: none;
      margin: 0;
      border: 0;
      border-radius: 0;
      box-shadow: none;
    }
    .box {
      padding: 0 16px calc(20px + env(safe-area-inset-bottom));
    }
    header {
      padding-top: calc(14px + env(safe-area-inset-top));
    }
    pre {
      max-height: 55dvh;
    }
  }
</style>
