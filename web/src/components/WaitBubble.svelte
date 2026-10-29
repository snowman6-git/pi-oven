<script lang="ts">
  import type { Chat } from '../lib/chat.svelte';
  import { runClock } from '../lib/util';

  // Shown where the answer will appear while the model has been asked and has not said anything yet: a spinner and what
  // is going on, as far as it is known. With the llama-infra package the wait before the first token is the prompt
  // being processed (prefill); a compaction or a retry says so; otherwise it is just waiting.
  // `text` replaces the usual wording (the start-up wait uses it).
  let { chat, text }: { chat: Chat; text?: string } = $props();

  let now = $state(Date.now());
  $effect(() => {
    now = Date.now();
    const t = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(t);
  });

  const label = $derived(
    text ? text : chat.compacting ? '컨텍스트 압축 중'
      : chat.retry ? `다시 시도하는 중 (${chat.retry})`
        : chat.speedSeen ? '프리필 중'
          : '대기 중',
  );
  const secs = $derived(chat.waitSince ? now - chat.waitSince : 0);
</script>

<div class="wait" role="status">
  <svg class="spin" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" aria-hidden="true">
    <circle cx="12" cy="12" r="9" stroke-opacity="0.2" />
    <path d="M21 12a9 9 0 0 0-9-9" />
  </svg>
  <span>{label}</span>
  {#if !text && secs >= 2000}<span class="t" aria-hidden="true">{runClock(secs)}</span>{/if}
</div>

<style>
  .wait {
    display: inline-flex;
    align-items: center;
    gap: 9px;
    color: var(--ink-2);
    font-size: 14px;
  }
  .spin {
    flex: none;
    color: var(--heat);
    animation: spin 0.9s linear infinite;
  }
  .t {
    color: var(--heat);
    font-variant-numeric: tabular-nums;
    font-size: 12.5px;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .spin {
      animation-duration: 3s;
    }
  }
</style>
