<script lang="ts">
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

  onMount(() => {
    // Follow growth (streaming, expanding tools) only while the reader is parked at the bottom.
    const ro = new ResizeObserver(() => stick && toBottom());
    ro.observe(inner);
    toBottom();
    return () => ro.disconnect();
  });
</script>

<div class="scroller" bind:this={scroller} onscroll={() => (stick = atBottom())}>
  <div class="inner" bind:this={inner}>
    {#if !chat.loaded && !chat.loadError}
      <div class="hint"><WaitBubble {chat} text={chat.link === 'open' ? 'pi를 시작하는 중' : S.connecting} /></div>
    {:else if chat.loadError}
      <p class="hint bad">{chat.loadError}</p>
    {:else if chat.items.length === 0}
      <section class="welcome">
        <h2 data-priv="paths">{baseName(chat.runtime.cwd)}</h2>
        <p class="path mono" data-priv="paths">{tildePath(chat.runtime.cwd, app.home)}</p>
        <p>이 폴더에서 pi가 파일을 읽고, 명령을 실행하고, 코드를 고칩니다. 아래에 요청을 입력하세요.</p>
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
