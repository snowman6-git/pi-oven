<script lang="ts">
  import type { Chat } from '../lib/chat.svelte';
  import { showcase } from '../lib/showcase.svelte';

  // While a file is in view the conversation is not: a line above the message box says what pi is doing,
  // and what it said when it finished. Pressing it brings the conversation back.
  let { chat }: { chat: Chat } = $props();

  const live = $derived.by(() => {
    for (let i = chat.items.length - 1; i >= 0; i--) {
      const it = chat.items[i];
      if (it.kind !== 'assistant') continue;
      for (let j = it.blocks.length - 1; j >= 0; j--) {
        const b = it.blocks[j];
        if (b.type === 'tool' && (b.status === 'running' || b.status === 'pending')) {
          const arg = typeof b.args?.path === 'string' ? b.args.path : typeof b.args?.command === 'string' ? b.args.command.split('\n')[0] : '';
          return `${b.name}${arg ? ' ' + arg.slice(0, 60) : ''}`;
        }
      }
      return '';
    }
    return '';
  });

  const reply = $derived.by(() => {
    for (let i = chat.items.length - 1; i >= 0; i--) {
      const it = chat.items[i];
      if (it.kind !== 'assistant') continue;
      for (const b of it.blocks) if (b.type === 'text' && b.text.trim()) return b.text.trim().split('\n')[0].slice(0, 140);
      return '';
    }
    return '';
  });

  const show = $derived(chat.running || (chat.unreadReply && !!reply));
</script>

{#if show}
  <button class="strip" type="button" class:run={chat.running} onclick={() => chat.editors.activate(null)} aria-live="polite" title="대화로 이동">
    <span class="dot" aria-hidden="true"></span>
    {#if chat.running}
      <span class="t"><b>pi 작업 중</b>{#if live}<span class="mono" data-priv="files"> {showcase.text(live)}</span>{/if}</span>
    {:else}
      <span class="t" data-priv="chat"><b>pi</b> {showcase.text(reply)}</span>
    {/if}
    <span class="go">대화 보기</span>
  </button>
{/if}

<style>
  .strip {
    display: flex;
    align-items: center;
    gap: 10px;
    width: min(100%, calc(var(--measure) + 74px));
    margin: 0 auto 8px;
    padding: 7px 12px;
    border: 1px solid var(--rule);
    border-radius: 10px;
    background: var(--raised);
    color: var(--ink-2);
    font-size: 13px;
    text-align: left;
  }
  .dot {
    flex: none;
    width: 8px;
    height: 8px;
    border-radius: 2px;
    background: var(--accent);
  }
  .run .dot {
    background: var(--heat);
    animation: strip-ember 1.2s ease-in-out infinite;
  }
  @keyframes strip-ember {
    50% { box-shadow: 0 0 0 4px color-mix(in srgb, var(--heat) 28%, transparent); }
  }
  .t {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .t b {
    color: var(--ink);
    font-weight: 650;
  }
  .go {
    flex: none;
    color: var(--ink-3);
    font-size: 12px;
  }
</style>
