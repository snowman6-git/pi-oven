<script lang="ts">
  import { t } from '../lib/i18n.svelte';
  import { onMount, tick } from 'svelte';
  import type { Chat } from '../lib/chat.svelte';
  import { S } from '../lib/strings';
  import { baseName, tildePath } from '../lib/util';
  import { app } from '../lib/app.svelte';
  import { SHOW } from '../lib/showin';
  import Icon from './Icon.svelte';
  import Turn from './Turn.svelte';
  import WaitBubble from './WaitBubble.svelte';

  let { chat }: { chat: Chat } = $props();

  let scroller: HTMLElement;
  let inner: HTMLElement;
  let stick = $state(true);

  // Index of each user message among user messages only, used to address pi's fork entries.
  // Counted from the start of the session, also when its older part is not loaded.
  const userIndex = $derived.by(() => {
    let n = chat.userBase;
    return chat.items.map((it) => (it.kind === 'user' ? n++ : -1));
  });

  // For each item: the user message its turn began with, and whether it is the last thing in that turn
  // (that is where the actions for the whole answer go: copy, regenerate).
  const turns = $derived.by(() => {
    let u = -1, n = chat.userBase;
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

  const maxTop = () => scroller.scrollHeight - scroller.clientHeight;
  const atBottom = () => maxTop() - scroller.scrollTop < 96;
  function toBottom(smooth = false) {
    scroller.scrollTo({ top: scroller.scrollHeight, behavior: smooth ? 'smooth' : 'auto' });
    if (!smooth) settle();
  }

  // A finger on the conversation, and where the last scroll left it. Within the last 96px the reader still counted as
  // parked, so a streaming answer snapped a phone's slow upward drag back down on every growth: it stuck and shook.
  // Now any upward move lets go at once, and nothing follows growth while a finger is down.
  //
  // Only the reader's move up lets go; reaching the end takes hold again. Whether the view is at the end is not asked
  // at other times: a finger held at the end while the answer grows (no following meanwhile), or the smooth scroll of
  // the jump button overtaken by the answer, leave the view short of the end without the reader having moved away, and
  // following then stopped for good.
  //
  // The browser moves the view up too: when the content gets shorter (a thought folds, a command's last lines go) the
  // view is pulled in, and if more arrives before that move is reported, it looks just like someone scrolling up. So a
  // move up counts only with the reader's hand in it, or when nothing changed its height since the view was last known.
  let touching = false;
  let lastTop = 0, lastH = 0, lastC = 0;
  function settle() {
    lastTop = scroller.scrollTop;
    lastH = scroller.scrollHeight;
    lastC = scroller.clientHeight;
  }
  /** When the reader last did something that scrolls (a wheel, a key, a pointer held down). */
  let handAt = -1e9;
  let held = false;
  const hand = () => touching || held || performance.now() - handAt < 800;
  /** A wheel going up holds the following back for a moment, so its first step is not overtaken by the answer. If the
      view does not move (nothing above, or the wheel went to a box inside), it never lets go. */
  let pausedTo = 0;
  let resume: ReturnType<typeof setTimeout> | undefined;
  function pause(ms: number) {
    pausedTo = Math.max(pausedTo, performance.now() + ms);
    clearTimeout(resume);
    resume = setTimeout(() => following() && toBottom(), pausedTo - performance.now() + 20);
  }
  /** A move up with no hand seen in it yet, and when: the view scrolls before the page hears of the wheel. */
  let doubtAt = -1e9;
  const following = () => stick && !touching && performance.now() >= pausedTo;
  /** Where the end was when the reader set out for it. A tool's output can move the end on while they are on the way;
      getting to where it was is getting there. */
  let aim = 0, aimAt = -1e9;
  let height = 0, heightWas = 0, grewAt = -1e9; // the last step the content grew by: too new to have been aimed past
  function setOut() {
    const now = performance.now();
    if (now - aimAt > 1200) { aim = now - grewAt < 150 ? heightWas : scroller.scrollHeight; aimAt = now; }
  }

  function onScroll() {
    const top = scroller.scrollTop;
    const gap = maxTop() - top;
    const layout = scroller.scrollHeight !== lastH || scroller.clientHeight !== lastC;
    if (top < lastTop - 1 && gap > 2) {
      if (hand() || !layout) stick = false;
      else if (stick) { // pulled in by the layout, it seems: back to the end, unless the wheel turns up late
        doubtAt = performance.now();
        pause(60);
      }
    } else if (atBottom()) stick = true;
    else if (top > lastTop && hand() && performance.now() - aimAt < 1200 && top + scroller.clientHeight > aim - 96) {
      stick = true;
      if (!touching) toBottom();
    }
    settle();
    if (top < NEAR_TOP) void older();
  }
  // The older part of a long session comes as the reader nears the top. What is put in front pushes the view down by
  // its height, so the view is moved back by as much: what the reader was looking at stays where it was.
  const NEAR_TOP = 900;
  async function older() {
    if (!chat.hasOlder || chat.loadingOlder) return;
    const h = scroller.scrollHeight;
    await chat.loadOlder();
    await tick();
    scroller.scrollTop += scroller.scrollHeight - h;
    settle();
  }
  // too little loaded to scroll at all (a short last turn): the next part comes without waiting for a scroll
  $effect(() => {
    void chat.items.length;
    if (chat.loaded && chat.hasOlder && !chat.loadingOlder) {
      void tick().then(() => { if (scroller.scrollHeight - scroller.clientHeight < NEAR_TOP) void older(); });
    }
  });

  function touchEnd() {
    if (!touching) return;
    touching = false;
    handAt = performance.now(); // a flick runs on after the finger
    // still holding on to the end: catch up with what came while the finger was down
    if (stick) toBottom();
  }

  onMount(() => {
    // Follow growth (streaming, expanding tools) only while the reader is parked at the bottom.
    // The view's own height too: the message box growing (a tray, chips, an attachment) covers the end.
    const ro = new ResizeObserver(() => {
      const h = scroller.scrollHeight;
      if (h > height) { heightWas = height; grewAt = performance.now(); }
      height = h;
      if (following()) toBottom();
      else if (scroller.scrollTop > maxTop() - 2) settle(); // pulled in, not scrolled
    });
    ro.observe(inner);
    ro.observe(scroller);
    toBottom();
    // passive: the browser must not wait on these before it scrolls
    const on = (type: string, f: (e: any) => void) => scroller.addEventListener(type, f, { passive: true });
    on('touchstart', (e: TouchEvent) => {
      touching = true;
      setOut();
      // the touched node may be redrawn mid-touch (a streaming answer is): the end of the touch then goes to the node
      // that is no longer in the page and never reaches the conversation, so it is listened for on the node itself
      const t = e.target;
      if (t instanceof Node && t !== scroller) {
        const end = () => { t.removeEventListener('touchend', end); t.removeEventListener('touchcancel', end); touchEnd(); };
        t.addEventListener('touchend', end, { passive: true });
        t.addEventListener('touchcancel', end, { passive: true });
      }
    });
    on('touchend', touchEnd);
    on('touchcancel', touchEnd);
    on('wheel', (e: WheelEvent) => {
      if (e.ctrlKey || Math.abs(e.deltaY) <= Math.abs(e.deltaX)) return; // zooming, or sideways
      handAt = performance.now();
      if (e.deltaY > 0) {
        setOut();
        // its scroll may have been reported first
        if (!stick && scroller.scrollTop + scroller.clientHeight > aim - 96) { stick = true; toBottom(); }
        return;
      }
      if (handAt - doubtAt < 100) stick = false;
      pause(200);
    });
    // the page scrolls the conversation by key also when the focus is on nothing in particular
    const key = (e: KeyboardEvent) => {
      if (!/^(Arrow(Up|Down)|Page(Up|Down)|Home|End| |Tab)$/.test(e.key) || (e.target as HTMLElement).closest?.('input, textarea, select, [contenteditable]')) return;
      handAt = performance.now();
      setOut();
    };
    window.addEventListener('keydown', key);
    on('pointerdown', (e: PointerEvent) => { if (e.pointerType !== 'touch') held = true; });
    // selecting text drags the view along, and the button may come up anywhere
    const up = () => (held = false);
    window.addEventListener('pointerup', up);
    window.addEventListener('pointercancel', up);
    on(SHOW, () => (stick = false));
    return () => {
      ro.disconnect();
      clearTimeout(resume);
      window.removeEventListener('keydown', key);
      window.removeEventListener('pointerup', up);
      window.removeEventListener('pointercancel', up);
    };
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

    {#if chat.loaded && chat.hasOlder}
      <p class="older">{chat.loadingOlder ? t('transcript.older') : t('transcript.olderMore')}</p>
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
  .older {
    margin: 4px 0 12px;
    text-align: center;
    font-size: 12.5px;
    color: var(--ink-3);
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
