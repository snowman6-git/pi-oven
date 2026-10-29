<script lang="ts">
  import type { Chat } from '../lib/chat.svelte';
  let { chat }: { chat: Chat } = $props();
</script>

<div class="toasts" role="status" aria-live="polite">
  {#each chat.toasts as t (t.id)}
    <p class={t.level}>{t.text}</p>
  {/each}
</div>

<style>
  .toasts {
    position: fixed;
    right: 16px;
    bottom: 16px;
    display: grid;
    gap: 8px;
    z-index: 50;
    max-width: min(380px, calc(100vw - 32px));
  }
  /* the same surface as the menus and dialogs, with a small square that says how serious it is */
  p {
    margin: 0;
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 10px 14px 10px 12px;
    border-radius: 10px;
    border: 1px solid var(--rule);
    background: var(--raised);
    color: var(--ink);
    font-size: 13.5px;
    line-height: 1.5;
    box-shadow: var(--shadow);
    overflow-wrap: anywhere;
    animation: in 0.18s ease-out;
  }
  p::before {
    content: '';
    flex: none;
    width: 8px;
    height: 8px;
    margin-top: 6px;
    border-radius: 2px;
    background: var(--ink-3);
  }
  p.info::before {
    background: var(--accent);
  }
  p.warning {
    border-color: color-mix(in srgb, var(--accent) 45%, var(--rule));
  }
  p.warning::before {
    background: var(--accent);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 25%, transparent);
  }
  p.error {
    border-color: color-mix(in srgb, var(--bad) 55%, var(--rule));
  }
  p.error::before {
    background: var(--bad);
  }
  @media (max-width: 720px) {
    .toasts {
      right: 12px;
      left: 12px;
      bottom: auto;
      top: calc(12px + env(safe-area-inset-top));
      max-width: none;
    }
  }
  @keyframes in {
    from {
      opacity: 0;
      transform: translateY(6px);
    }
  }
</style>
