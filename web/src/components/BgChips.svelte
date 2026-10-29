<script lang="ts">
  import type { Chat } from '../lib/chat.svelte';
  import type { BgTask } from '../lib/protocol';
  import { runClock } from '../lib/util';

  let { chat }: { chat: Chat } = $props();

  // Chips sit side by side in one row. How many fit follows the row's width; the rest go to the circle.
  let rowW = $state(0);
  const CHIP_MIN = 150; // narrowest a chip may get before its name is no use
  const fit = $derived(rowW ? Math.max(1, Math.floor((rowW - 36) / (CHIP_MIN + 6))) : 4);

  // A running task shows how long it has been running (the server's start time, so a reload or another device
  // carries on from the same number). Clocks that differ by more than the task's age can't be told apart here.
  let now = $state(Date.now());
  const anyRunning = $derived(chat.bgChips.some((t) => t.status === 'running'));
  $effect(() => {
    if (!anyRunning) return;
    now = Date.now();
    const t = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(t);
  });
  const elapsed = (t: BgTask) => (typeof t.startTime === 'number' ? Math.max(0, now - t.startTime) : 0);
</script>

{#if chat.bgChips.length}
  {@const chips = chat.bgChips}
  {@const rest = chips.slice(fit).filter((t) => !chat.bgLanding.includes(t.id))}
  <div class="bgrow" role="group" aria-label="백그라운드 작업" bind:clientWidth={rowW}>
    {#each chips.slice(0, fit) as t (t.id)}
      <span class="bgchip" data-bg-chip={t.id} class:land={chat.bgLanding.includes(t.id)} class:fading={chat.bgFading.includes(t.id)} class:run={t.status === 'running'} class:bad={t.status === 'failed' || t.status === 'killed'}>
        <button class="main" type="button" onclick={() => { chat.bgFocus = t.id; chat.bgOpen = true; chat.dismissBg(t.id); }} aria-haspopup="dialog"
          title={t.command.split('\n')[0]}>
          <span class="dot" role="img" aria-label={t.status === 'running' ? '실행 중' : t.status === 'completed' ? '완료' : t.status === 'killed' ? '중단됨' : '실패'}></span>
          <span class="nm">{t.name || t.description || t.command.split('\n')[0]}</span>
          {#if t.status === 'running' && elapsed(t) >= 1000}
            <span class="st tm">{runClock(elapsed(t))}</span>
          {:else}
            <span class="st">{t.status === 'running' ? '실행 중' : t.status === 'completed' ? '완료' : t.status === 'killed' ? '중단됨' : '실패'}</span>
          {/if}
        </button>
        {#if t.status !== 'running'}
          <button class="x" type="button" aria-label="{t.name || '작업'} 알림 닫기" onclick={() => chat.dismissBg(t.id)}>×</button>
        {/if}
      </span>
    {/each}
    <!-- What does not fit in the one row piles up in one small circle. It is there (hidden) before the first one is stacked, so a task has somewhere to land. -->
    {#if chips.length > fit}
      <button class="stack" data-bg-stack class:land={rest.length === 0} class:run={rest.some((t) => t.status === 'running')} class:bad={rest.some((t) => t.status === 'failed' || t.status === 'killed')}
        type="button" aria-label="작업 {rest.length}개 더 보기" title="작업 {rest.length}개 더 보기" aria-haspopup="dialog" onclick={() => (chat.bgOpen = true)}>+{rest.length}</button>
    {/if}
  </div>
{/if}

<style>
  .bgrow {
    max-width: calc(var(--measure) + 74px);
    margin: 0 auto 8px;
  }
  .bgrow {
    display: flex;
    flex-wrap: nowrap;
    align-items: center;
    gap: 6px;
    width: 100%;
  }
  .bgchip {
    display: inline-flex;
    align-items: center;
    flex: 0 1 auto;
    min-width: 0;
    max-width: 220px;
    height: 30px;
    border-radius: 999px;
    border: 1px solid var(--rule);
    background: var(--raised);
    color: var(--ink-2);
    font-size: 13px;
    font-weight: 560;
  }
  .bgchip.land {
    visibility: hidden;
  }
  .bgchip.fading {
    opacity: 0;
    scale: 0.9;
    transition: opacity 0.3s, scale 0.3s;
  }
  .bgchip .main {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    height: 100%;
    padding: 0 12px 0 10px;
    border-radius: 999px;
  }
  .bgchip .nm {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .bgchip .st {
    flex: none;
    color: var(--ink-3);
    font-weight: 500;
  }
  /* amber: only while the task is actually running */
  .bgchip .st.tm {
    color: var(--heat);
    font-variant-numeric: tabular-nums;
    font-weight: 560;
  }
  .bgchip .x {
    flex: none;
    width: 24px;
    height: 24px;
    margin-right: 3px;
    border-radius: 50%;
    color: var(--ink-3);
    font-size: 16px;
    line-height: 1;
  }
  .bgchip .x:hover {
    background: var(--rule);
  }
  /* One circle for everything that does not fit, same height as a chip so it is a true circle. */
  .stack {
    flex: none;
    width: 30px;
    height: 30px;
    border-radius: 50%;
    border: 1px solid var(--rule);
    background: var(--raised);
    color: var(--ink-2);
    font-size: 12px;
    font-weight: 650;
    display: grid;
    place-items: center;
    font-variant-numeric: tabular-nums;
  }
  .stack.land {
    visibility: hidden;
  }
  .stack.run {
    color: var(--ink);
    border-color: color-mix(in srgb, var(--heat) 50%, var(--rule));
  }
  .stack.bad {
    border-color: color-mix(in srgb, var(--bad) 60%, var(--rule));
  }
  .bgchip .dot {
    flex: none;
    width: 8px;
    height: 8px;
    border-radius: 2px;
    background: var(--ok);
  }
  .bgchip.run {
    color: var(--ink);
    border-color: color-mix(in srgb, var(--heat) 50%, var(--rule));
  }
  .bgchip.run .dot {
    background: var(--heat);
    animation: bg-ember 1.2s ease-in-out infinite;
  }
  .bgchip.bad .dot {
    background: var(--bad);
  }
  @keyframes bg-ember {
    50% {
      box-shadow: 0 0 0 4px color-mix(in srgb, var(--heat) 28%, transparent);
    }
  }
</style>
