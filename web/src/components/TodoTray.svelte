<script lang="ts">
  import { headline, summarize, unfinished, type Task } from '../addons/todo';
  import type { Chat } from '../lib/chat.svelte';
  import Icon from './Icon.svelte';

  // The model's task list, just above the message box. Folded it is one line: a square for each task, what is
  // happening now, and how far along. Opened it is the whole list. It only tells; the model owns the list.
  let { chat }: { chat: Chat } = $props();

  const s = $derived(summarize(chat.todo ?? []));
  const key = $derived(s.tasks.map((t) => `${t.id}:${t.status}`).join(','));
  let open = $state(read());
  let hiddenAt = $state<string | null>(null);
  let list = $state<HTMLElement>();

  function read() { try { return localStorage.getItem('pi-oven.todo-open') === 'on'; } catch { return false; } }
  function toggle() {
    open = !open;
    try { localStorage.setItem('pi-oven.todo-open', open ? 'on' : 'off'); } catch { /* private mode */ }
  }

  // Hiding it lasts until the list changes: a new task or a finished one brings it back.
  const show = $derived(chat.todo !== null && s.total > 0 && hiddenAt !== key);

  const squares = $derived(s.total <= 12);
  const label = (t: Task) => (t.status === 'completed' ? '완료' : t.status === 'in_progress' ? '진행 중' : '대기');

  // A task that has just finished pops its square, the way a background task's chip does.
  let before = new Map<number, string>();
  let ready = false;
  $effect(() => {
    const now = new Map(s.tasks.map((t) => [t.id, t.status]));
    const finished = ready ? s.tasks.filter((t) => t.status === 'completed' && before.get(t.id) && before.get(t.id) !== 'completed') : [];
    before = now;
    ready = true;
    if (!finished.length || matchMedia('(prefers-reduced-motion: reduce)').matches) return;
    requestAnimationFrame(() => {
      for (const t of finished) {
        for (const el of document.querySelectorAll<HTMLElement>(`[data-todo-seg="${t.id}"]`)) {
          el.animate([{ transform: 'scale(1)' }, { transform: 'scale(1.7)', offset: 0.3 }, { transform: 'scale(0.9)', offset: 0.6 }, { transform: 'scale(1)' }], { duration: 640, easing: 'cubic-bezier(0.3, 1.4, 0.5, 1)' });
          el.animate([{ boxShadow: '0 0 0 0 color-mix(in srgb, var(--ok) 60%, transparent)' }, { boxShadow: '0 0 0 8px color-mix(in srgb, var(--ok) 0%, transparent)' }], { duration: 640, easing: 'ease-out' });
        }
      }
    });
  });

  // Opened, the row being worked on stays in view.
  $effect(() => {
    key;
    if (open) queueMicrotask(() => list?.querySelector('[data-now="true"]')?.scrollIntoView({ block: 'nearest' }));
  });
</script>

{#if show}
  <div class="todo" class:open class:done={s.allDone} role="region" aria-label="할 일">
    <div class="head">
      <button class="bar" type="button" aria-expanded={open} aria-controls="todo-list" onclick={toggle}>
        {#if squares}
          <span class="segs" aria-hidden="true">
            {#each s.tasks as t (t.id)}<i class="seg {t.status}" data-todo-seg={t.id}></i>{/each}
          </span>
        {:else}
          <span class="meter" aria-hidden="true"><i class="ok" style="width:{(s.done / s.total) * 100}%"></i><i class="now" style="width:{s.current?.status === 'in_progress' ? Math.max(3, 100 / s.total) : 0}%"></i></span>
        {/if}
        {#if open}<b class="title">할 일</b>{:else}<span class="hl">{headline(s)}</span>{/if}
        <span class="n">{s.done} / {s.total}</span>
        <span class="chev" class:down={open}><Icon name="up" size={14} /></span>
      </button>
      <button class="x" type="button" aria-label="할 일 숨기기" title="목록이 바뀔 때까지 숨깁니다" onclick={() => (hiddenAt = key)}><Icon name="x" size={14} /></button>
    </div>
    {#if open}
      <ul id="todo-list" bind:this={list}>
        {#each s.tasks as t (t.id)}
          {@const wait = t.status === 'pending' ? unfinished(t, s.tasks) : []}
          <li class="row {t.status}" class:blocked={wait.length > 0} data-now={t.status === 'in_progress'} title={t.description}>
            <span class="mark" role="img" aria-label={label(t)}>
              {#if t.status === 'completed'}<svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M5 12.5l4.5 4.5L19 7.5"/></svg>{/if}
            </span>
            <span class="sj">{t.subject}</span>
            {#if t.status === 'in_progress'}<span class="side now">{t.activeForm || '진행 중'}</span>
            {:else if wait.length}<span class="side">{wait.map((id) => `#${id}`).join(' ')} 끝나면</span>{/if}
          </li>
        {/each}
      </ul>
    {/if}
  </div>
{/if}

<style>
  .todo {
    max-width: calc(var(--measure) + 74px);
    margin: 0 auto 8px;
    border: 1px solid var(--rule);
    border-radius: 10px;
    background: var(--raised);
    overflow: hidden;
  }
  .todo.open {
    border-radius: 12px;
  }
  .head {
    display: flex;
    align-items: center;
  }
  .bar {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 12px;
    height: 38px;
    padding: 0 6px 0 14px;
    text-align: left;
  }
  .todo.open .head {
    border-bottom: 1px solid var(--rule);
  }
  .segs {
    display: flex;
    gap: 3px;
    flex: none;
  }
  .seg {
    width: 7px;
    height: 14px;
    border-radius: 2px;
    background: var(--rule);
  }
  .seg.completed {
    background: var(--ok);
  }
  .seg.in_progress {
    background: var(--heat);
    animation: todo-ember 1.2s ease-in-out infinite;
  }
  @keyframes todo-ember {
    50% { box-shadow: 0 0 0 3px color-mix(in srgb, var(--heat) 26%, transparent); }
  }
  @media (prefers-reduced-motion: reduce) {
    .seg.in_progress { animation: none; }
  }
  .meter {
    flex: none;
    width: 96px;
    height: 8px;
    border-radius: 2px;
    background: var(--rule);
    display: flex;
    overflow: hidden;
  }
  .meter .ok { background: var(--ok); }
  .meter .now { background: var(--heat); }
  .hl,
  .title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .hl {
    font-weight: 560;
  }
  .title {
    font-weight: 700;
    font-size: 13px;
  }
  .done .hl {
    color: var(--ok);
  }
  .n {
    flex: none;
    font-size: 12.5px;
    color: var(--ink-2);
    font-variant-numeric: tabular-nums;
  }
  .chev {
    flex: none;
    color: var(--ink-3);
    display: inline-grid;
    transition: rotate 0.14s;
  }
  .chev.down {
    rotate: 180deg;
  }
  .x {
    flex: none;
    width: 30px;
    height: 30px;
    margin-right: 4px;
    border-radius: 7px;
    display: inline-grid;
    place-items: center;
    color: var(--ink-3);
    opacity: 0;
  }
  .todo:hover .x,
  .x:focus-visible {
    opacity: 1;
  }
  @media (hover: none) {
    .x { opacity: 0.6; }
  }
  @media (hover: hover) {
    .x:hover { background: var(--rule); color: var(--ink); }
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 6px 8px 8px;
    display: grid;
    gap: 1px;
    max-height: min(40dvh, 320px);
    overflow-y: auto;
    scrollbar-width: none;
  }
  ul::-webkit-scrollbar {
    display: none;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 5px 6px;
    border-radius: 8px;
    min-width: 0;
  }
  .mark {
    flex: none;
    width: 15px;
    height: 15px;
    border-radius: 4px;
    box-sizing: border-box;
    border: 1.5px solid var(--ink-3);
    display: inline-grid;
    place-items: center;
  }
  .row.blocked .mark {
    border-style: dashed;
  }
  .row.completed .mark {
    border: 0;
    background: var(--ok);
    color: #10170f;
  }
  .row.in_progress {
    background: color-mix(in srgb, var(--heat) 9%, transparent);
    padding: 7px 8px;
  }
  .row.in_progress .mark {
    border: 0;
    background: var(--heat);
    box-shadow: 0 0 0 4px color-mix(in srgb, var(--heat) 22%, transparent);
  }
  .sj {
    flex: 1;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .row.completed .sj {
    color: var(--ink-2);
    text-decoration: line-through;
    text-decoration-color: var(--ink-3);
  }
  .row.in_progress .sj {
    font-weight: 650;
  }
  .row.blocked .sj {
    color: var(--ink-2);
  }
  .side {
    flex: none;
    max-width: 45%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 11.5px;
    color: var(--ink-2);
  }
  .side.now {
    color: var(--heat);
    font-size: 12px;
  }
</style>
