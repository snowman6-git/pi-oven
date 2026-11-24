<script lang="ts">
  import { t } from '../lib/i18n.svelte';
  import { onMount } from 'svelte';
  import type { Chat } from '../lib/chat.svelte';
  import { S } from '../lib/strings';
  import { baseName, tildePath } from '../lib/util';
  import { app } from '../lib/app.svelte';
  import Icon from './Icon.svelte';
  import Turn from './Turn.svelte';
  import WaitBubble from './WaitBubble.svelte';

  let { chat }: { chat: Chat } = $props();

  let scroller: HTMLElement;
  let inner: HTMLElement;
  let stick = $state(true);

  // Index of each user message among user messages only, used to address pi's fork entries.
  const userIndex = $derived.by(() => {
    let n = 0;
    return chat.items.map((it) => (it.kind === 'user' ? n++ : -1));
  });

  // For each item: the user message its turn began with, and whether it is the last thing in that turn
  // (that is where the actions for the whole answer go: copy, regenerate).
  const turns = $derived.by(() => {
    let u = -1, n = 0;
    return chat.items.map((it, i) => {
      if (it.kind === 'user') u = n++;
      const next = chat.items[i + 1];
      return { user: u, end: !next || next.kind === 'user' };
    });
  });

  // While the last message is the model's own and still open, its turn shows the waiting spinner itself.
  const lastIsLive = $derived.by(() => {
    const last = chat.items[chat.items.length - 1];
    return last?.kind === 'assistant' && last.live;
  });

  function atBottom() {
    return scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight < 96;
  }
  const toBottom = (smooth = false) => scroller.scrollTo({ top: scroller.scrollHeight, behavior: smooth ? 'smooth' : 'auto' });

  // A finger on the conversation, and where the last scroll left it. Within the last 96px the reader still counted as
  // parked, so a streaming answer snapped a phone's slow upward drag back down on every growth: it stuck and shook.
  // Now any upward move lets go at once, and nothing follows growth while a finger is down.
  let touching = false;
  let lastTop = 0;
  function onScroll() {
    const top = scroller.scrollTop;
    const gap = scroller.scrollHeight - top - scroller.clientHeight;
    // up, and not just the browser pulling the view in because the content got shorter (that leaves it at the end)
    if (top < lastTop - 1 && gap > 2) stick = false;
    else if (!touching) stick = atBottom();
    lastTop = top;
  }
  function touchEnd() {
    touching = false;
    stick = atBottom();
  }

  onMount(() => {
    // Follow growth (streaming, expanding tools) only while the reader is parked at the bottom.
    const ro = new ResizeObserver(() => stick && !touching && toBottom());
    ro.observe(inner);
    toBottom();
    // passive: the browser must not wait on these before it scrolls
    const on = (type: string, f: (e: any) => void) => scroller.addEventListener(type, f, { passive: true });
    on('touchstart', () => (touching = true));
    on('touchend', touchEnd);
    on('touchcancel', touchEnd);
    on('wheel', (e: WheelEvent) => { if (e.deltaY < 0) stick = false; });
    return () => ro.disconnect();
  });
</script>

<div class="scroller" bind:this={scroller} onscroll={onScroll}>
  <div class="inner" bind:this={inner}>
    {#if !chat.loaded && !chat.loadError}
      <div class="hint"><WaitBubble {chat} text={chat.link === 'open' ? t('transcript.starting') : S.connecting} /></div>
    {:else if chat.loadError}
      <p class="hint bad">{chat.loadError}</p>
    {:else if chat.items.length === 0}
      <section class="welcome">
        <h2 data-priv="paths">{baseName(chat.runtime.cwd)}</h2>
        <p class="path mono" data-priv="paths">{tildePath(chat.runtime.cwd, app.home)}</p>
        <p>{t('transcript.intro')}</p>
      </section>
    {/if}

    {#each chat.items as item, i (item.key)}
      <Turn {item} {chat} userIndex={userIndex[i]} turn={turns[i]} />
    {/each}
    <!-- the model has been asked and nothing has come back yet (once its message starts, the turn itself shows this) -->
    {#if chat.waiting && !lastIsLive}
      <div class="waitrow"><WaitBubble {chat} /></div>
    {/if}
  </div>
</div>

{#if !stick}
  <button class="jump" type="button" onclick={() => (toBottom(true), (stick = true))}>
    <Icon name="down" size={16} />{S.jumpToLatest}
  </button>
{/if}

<style>
  .scroller {
    flex: 1;
    min-height: 0;
    container: chat / inline-size;
    /* no scroll bar: it would sit between the text and the edge and push the text off the message box's centre. Wheel, touch and keys all still scroll. */
    scrollbar-width: none;
    overflow-y: auto;
    overflow-anchor: none;
    padding: 12px 24px 28px;
    scroll-padding-bottom: 24px;
  }
  .inner {
    max-width: calc(var(--measure) + 74px);
    margin: 0 auto;
  }
  .waitrow {
    padding: 14px 0;
  }
  .hint {
    color: var(--ink-3);
    padding: 40px 0;
    text-align: center;
  }
  .bad {
    color: var(--bad);
  }
  .welcome {
    padding: 9vh 0 4vh;
    max-width: 38rem;
    container-type: inline-size;
  }
  .welcome h2 {
    margin: 0;
    font-size: clamp(24px, 9.5cqw, 56px);
    line-height: 1.1;
    word-break: keep-all;
    letter-spacing: -0.035em;
    font-weight: 750;
    overflow-wrap: anywhere;
  }
  .path {
    margin: 10px 0 22px;
    color: var(--ink-3);
    overflow-wrap: anywhere;
  }
  .welcome p:last-child {
    color: var(--ink-2);
    margin: 0;
  }
  .jump {
    position: absolute;
    left: 50%;
    bottom: 12px;
    transform: translateX(-50%);
    display: inline-flex;
    gap: 4px;
    align-items: center;
    height: 32px;
    padding: 0 14px 0 10px;
    border-radius: 999px;
    border: 1px solid var(--rule);
    background: var(--raised);
    color: var(--ink);
    font-size: 13px;
    font-weight: 560;
    box-shadow: var(--shadow);
    z-index: 3;
  }
  .scroller::-webkit-scrollbar {
    display: none;
  }
  @media (max-width: 720px) {
    .scroller {
      padding: 8px 14px 24px; /* the same side space as the message box (Composer's .dock) */
    }
    .welcome {
      padding: 7vh 6px 3vh;
    }
  }
</style>
