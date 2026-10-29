<script lang="ts">
  import { onMount } from 'svelte';
  import { app } from './lib/app.svelte';
  import { FILE_MIME } from './lib/editor.svelte';
  import { showcase } from './lib/showcase.svelte';
  import { maskExample } from './lib/maskdom';
  import { S } from './lib/strings';
  import AgentStrip from './components/AgentStrip.svelte';
  import Composer from './components/Composer.svelte';
  import BgFlight from './components/BgFlight.svelte';
  import ConfirmDialog from './components/ConfirmDialog.svelte';
  import ContextMenu from './components/ContextMenu.svelte';
  import ExtensionDialog from './components/ExtensionDialog.svelte';
  import Header from './components/Header.svelte';
  import Icon from './components/Icon.svelte';
  import Inspector from './components/Inspector.svelte';
  import Sidebar from './components/Sidebar.svelte';
  import ServerDown from './components/ServerDown.svelte';
  import TokenGate from './components/TokenGate.svelte';
  import Toasts from './components/Toasts.svelte';
  import Transcript from './components/Transcript.svelte';

  onMount(() => void app.boot());

  const chat = $derived(app.current);

  // The middle holds the conversation and the editor: side by side when there is room (and it is switched on), else one at a time in tabs.
  let mainEl = $state<HTMLElement>();
  let mainW = $state(1000);
  const canSplit = $derived(mainW >= 860);
  // a flight to a bubble in the conversation only makes sense while the conversation can be seen
  $effect(() => { if (chat) chat.chatVisible = (chat.editors.split && chat.editors.docs.length > 0 && mainW >= 860) || chat.editors.active === null; });
  const splitOn = $derived(!!chat && chat.editors.split && chat.editors.docs.length > 0 && canSplit);

  function gripSplit(e: PointerEvent, chat: NonNullable<typeof app.current>) {
    const el = e.currentTarget as HTMLElement;
    el.setPointerCapture(e.pointerId);
    const move = (m: PointerEvent) => {
      const r = mainEl!.getBoundingClientRect();
      chat.editors.setSplitFrac((m.clientX - r.left) / r.width);
    };
    const up = () => {
      el.removeEventListener('pointermove', move);
      el.removeEventListener('pointerup', up);
      chat.editors.setSplitFrac(chat.editors.splitFrac, true);
    };
    el.addEventListener('pointermove', move);
    el.addEventListener('pointerup', up);
    e.preventDefault();
  }
  function gripSplitKey(e: KeyboardEvent, chat: NonNullable<typeof app.current>) {
    const d = e.key === 'ArrowLeft' ? -0.03 : e.key === 'ArrowRight' ? 0.03 : 0;
    if (!d) return;
    e.preventDefault();
    chat.editors.setSplitFrac(chat.editors.splitFrac + d, true);
  }

  // A file dragged from the explorer onto the middle opens in the editor.
  let dropping = $state(false);
  const carriesFile = (e: DragEvent) => !!e.dataTransfer?.types.includes(FILE_MIME);
  function dragOver(e: DragEvent) {
    if (!carriesFile(e)) return;
    e.preventDefault();
    e.dataTransfer!.dropEffect = 'copy';
    // over the message box it means "put the path in the message", not "open it"
    dropping = !(e.target as HTMLElement).closest('.composer');
  }
  function dragLeave(e: DragEvent) {
    if (!(e.currentTarget as HTMLElement).contains(e.relatedTarget as Node | null)) dropping = false;
  }
  function dropFile(e: DragEvent, chat: NonNullable<typeof app.current>) {
    if (!carriesFile(e)) return;
    e.preventDefault();
    dropping = false;
    try {
      const { path, runtime } = JSON.parse(e.dataTransfer!.getData(FILE_MIME));
      if (runtime === chat.runtime.id && typeof path === 'string') void chat.editors.open(path);
    } catch { /* not ours after all */ }
  }

  // A click on the dimmed space around an open dialog closes it, like Escape does. Only a press that starts there
  // counts, so selecting text inside and letting go outside does not.
  let pressedOnBackdrop: HTMLDialogElement | null = null;
  function backdropDown(e: PointerEvent) {
    const d = e.target;
    pressedOnBackdrop = d instanceof HTMLDialogElement && d.open && !inside(d, e) ? d : null;
  }
  function backdropClick(e: MouseEvent) {
    const d = e.target;
    if (!(d instanceof HTMLDialogElement) || !d.open || d !== pressedOnBackdrop || inside(d, e)) return;
    pressedOnBackdrop = null;
    // the same path as Escape: a dialog that answers a question (cancel) hears about it
    const cancel = new Event('cancel', { cancelable: true });
    d.dispatchEvent(cancel);
    if (!cancel.defaultPrevented) d.close();
  }
  const inside = (d: HTMLElement, e: MouseEvent) => {
    const r = d.getBoundingClientRect();
    return e.clientX >= r.left && e.clientX <= r.right && e.clientY >= r.top && e.clientY <= r.bottom;
  };

  // Unsaved edits are worth a warning before the tab goes away.
  function beforeUnload(e: BeforeUnloadEvent) {
    if (app.chats.some((c) => c.editors.dirtyCount > 0)) e.preventDefault();
  }

  // Drag the left edge of the right panel to resize it; the width is remembered.
  function grab(e: PointerEvent) {
    const el = e.currentTarget as HTMLElement;
    const startX = e.clientX;
    const startW = app.inspectorW;
    el.setPointerCapture(e.pointerId);
    const move = (m: PointerEvent) => app.setInspectorWidth(startW + (startX - m.clientX));
    const up = () => {
      el.removeEventListener('pointermove', move);
      el.removeEventListener('pointerup', up);
      app.setInspectorWidth(app.inspectorW, true);
    };
    el.addEventListener('pointermove', move);
    el.addEventListener('pointerup', up);
    e.preventDefault();
  }
  function gripKey(e: KeyboardEvent) {
    const d = e.key === 'ArrowLeft' ? 24 : e.key === 'ArrowRight' ? -24 : 0;
    if (!d) return;
    e.preventDefault();
    app.setInspectorWidth(app.inspectorW + d, true);
  }

  // Showcase mode: the page says which kinds are hidden (the stylesheet blurs them), and the text drawn knows the folders to hide.
  $effect(() => {
    const a = showcase.attr;
    if (a) document.documentElement.dataset.showcase = a;
    else delete document.documentElement.dataset.showcase;
    if (a && showcase.style === 'example') document.documentElement.dataset.showcaseStyle = 'example';
    else delete document.documentElement.dataset.showcaseStyle;
  });
  $effect(() => {
    maskExample(showcase.masked);
  });
  $effect(() => {
    showcase.cwds = [...new Set([...app.chats.map((c) => c.runtime.cwd), ...app.sessions.map((s) => s.cwd)])];
    showcase.home = app.home;
  });

  $effect(() => {
    const base = chat?.title && !showcase.active('titles') ? `${chat.title.slice(0, 40)} — pi-oven` : 'pi-oven';
    document.title = app.unseenCount ? `(${app.unseenCount}) ${base}` : base;
  });
</script>

<svelte:window onbeforeunload={beforeUnload} onpointerdowncapture={backdropDown} onclick={backdropClick} />

{#if app.status === 'auth'}
  <TokenGate />
{:else if app.status === 'down'}
  <ServerDown />
{:else if app.status === 'booting'}
  <main class="center"><p class="faint">{S.connecting}</p></main>
{:else}
  <div class="shell" class:inspector={!app.narrow && app.inspectorOpen && chat} class:rail={app.rail} class:anim={app.foldAnim} data-sidebar={app.sidebarOpen ? 'open' : 'closed'}
    style:--inspector-w="{app.inspectorW}px">
    <div class="side"><Sidebar /></div>

    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <section class="main" class:has-chat={!!chat} class:split={splitOn} bind:this={mainEl} bind:clientWidth={mainW} style:--frac={chat?.editors.splitFrac}
      ondragover={(e) => dragOver(e)} ondragleave={(e) => dragLeave(e)} ondrop={(e) => chat && dropFile(e, chat)}>
      {#if chat}
        {#key chat.runtime.id}
          <div class="g-head"><Header {chat} /></div>
          {#if chat.editors.docs.length}<div class="g-tabs">{#await import('./components/EditorTabs.svelte') then m}<m.default {chat} {splitOn} {canSplit} />{/await}</div>{/if}
          <div class="g-chat" class:behind={!splitOn && chat.editors.active !== null}><Transcript {chat} /></div>
          {#if chat.editors.docs.length}<div class="g-pane">{#await import('./components/EditorPane.svelte') then m}<m.default {chat} {splitOn} />{/await}</div>{/if}
          {#if splitOn}
            <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
            <div class="g-grip" role="separator" aria-orientation="vertical" aria-label="대화와 편집기 폭" aria-valuenow={Math.round(chat.editors.splitFrac * 100)} aria-valuemin="30" aria-valuemax="70" tabindex="0"
              onpointerdown={(e) => gripSplit(e, chat)} onkeydown={(e) => gripSplitKey(e, chat)} ondblclick={() => chat.editors.setSplitFrac(0.5, true)}></div>
          {/if}
          {#if dropping}<div class="dropzone" aria-hidden="true"><span>여기에 놓으면 편집기로 엽니다</span></div>{/if}
          <div class="g-bottom" class:compact={!splitOn && chat.editors.active !== null}>
            {#if !splitOn && chat.editors.active !== null}<AgentStrip {chat} />{/if}
            <Composer {chat} />
          </div>
          <ExtensionDialog {chat} />
          {#if chat.bgOpen}{#await import('./components/BackgroundTasks.svelte') then m}<m.default {chat} />{/await}{/if}
          <BgFlight {chat} />
          <Toasts {chat} />
        {/key}
      {:else}
        <header class="bare">
          <button class="icon-btn menu" type="button" aria-label="세션 목록" onclick={() => (app.sidebarOpen = true)}><Icon name="menu" /></button>
        </header>
        <div class="empty">
          <h1>{S.emptyTitle}</h1>
          <p>{S.emptyBody}</p>
          <button class="btn primary" type="button" onclick={() => (app.folderPicker = true)}><Icon name="folder" size={16} />{S.openFolder}</button>
        </div>
      {/if}
      {#if app.error}
        <div class="error" role="alert">
          <span>{app.error}</span>
          <button class="icon-btn" type="button" aria-label="닫기" onclick={() => (app.error = null)}><Icon name="x" size={16} /></button>
        </div>
      {/if}
    </section>

    {#if app.showInspector && chat}
      {#if app.narrow}
        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
        <div class="scrim on" onclick={() => (app.inspectorDrawer = false)}></div>
      {/if}
      <div class="insp" class:drawer={app.narrow} class:wide={app.inspectorTab !== 'info'}>
        {#if !app.narrow}
          <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
          <div class="grip" role="separator" aria-orientation="vertical" aria-label="패널 너비" aria-valuenow={app.inspectorW} aria-valuemin="280" aria-valuemax="760" tabindex="0"
            onpointerdown={grab} onkeydown={gripKey} ondblclick={() => app.setInspectorWidth(340, true)}></div>
        {/if}
        <Inspector {chat} />
      </div>
    {/if}
  </div>

  {#if app.folderPicker}{#await import('./components/FolderPicker.svelte') then m}<m.default />{/await}{/if}
  {#if app.settingsOpen}{#await import('./components/SettingsDialog.svelte') then m}<m.default />{/await}{/if}
  {#if app.modelDialog && chat}{#await import('./components/ModelDialog.svelte') then m}<m.default {chat} />{/await}{/if}
  {#if app.treeTarget}{#await import('./components/TreePanel.svelte') then m}<m.default target={app.treeTarget} />{/await}{/if}
  <ConfirmDialog />
  <ContextMenu />
  {#if app.flashText}<div class="flash" role="status">{app.flashText}</div>{/if}
{/if}

<style>
  .center {
    height: 100%;
    display: grid;
    place-content: center;
    justify-items: center;
    gap: 14px;
    color: var(--ink-2);
  }
  .faint {
    color: var(--ink-3);
  }
  .shell {
    height: 100%;
    display: grid;
    grid-template-columns: var(--sidebar-w) minmax(0, 1fr);
  }
  .shell.inspector {
    grid-template-columns: var(--sidebar-w) minmax(0, 1fr) var(--inspector-w);
  }
  .side,
  .insp {
    min-height: 0;
    height: 100%;
    overflow: hidden;
  }
  /* Only while the list folds or unfolds; dragging the right panel must follow the pointer exactly. */
  .shell.anim {
    transition: grid-template-columns 0.16s ease-out;
  }
  .shell.rail,
  .shell.rail.inspector {
    --sidebar-w: 56px;
  }
  @media (prefers-reduced-motion: reduce) {
    .shell.anim { transition: none; }
  }
  .insp:not(.drawer) {
    position: relative;
    overflow: visible;
  }
  .insp:not(.drawer) > :global(aside) {
    overflow: hidden;
  }
  .grip {
    position: absolute;
    left: -3px;
    top: 0;
    bottom: 0;
    width: 7px;
    cursor: col-resize;
    z-index: 5;
    touch-action: none;
  }
  .grip:hover,
  .grip:focus-visible,
  .grip:active {
    background: linear-gradient(to right, transparent 2px, var(--heat) 2px, var(--heat) 4px, transparent 4px);
    outline: none;
  }
  .main {
    position: relative;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    height: 100%;
  }
  /* With a session: a grid, so the conversation and the editor can stack in one place (tabs) or sit side by side (split) without being rebuilt. */
  .main.has-chat {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    grid-template-rows: auto auto minmax(0, 1fr) auto;
  }
  .g-head { grid-row: 1; grid-column: 1 / -1; min-width: 0; }
  .g-tabs { grid-row: 2; grid-column: 1 / -1; min-width: 0; }
  .g-chat, .g-pane { grid-row: 3; grid-column: 1 / -1; min-width: 0; min-height: 0; display: flex; flex-direction: column; }
  /* the "jump to latest" button is placed against the conversation, not the whole column (which includes the message box) */
  .g-chat { position: relative; }
  .g-bottom { grid-row: 4; grid-column: 1 / -1; min-width: 0; }
  .g-chat.behind { visibility: hidden; pointer-events: none; }
  .main.split {
    grid-template-columns: minmax(300px, calc((100% - 7px) * var(--frac, 0.5))) 7px minmax(320px, 1fr);
  }
  .main.split .g-chat { grid-row: 2 / 4; grid-column: 1; }
  .main.split .g-tabs { grid-row: 2; grid-column: 3; }
  .main.split .g-pane { grid-row: 3 / 5; grid-column: 3; }
  .main.split .g-bottom { grid-row: 4; grid-column: 1; }
  .g-grip {
    grid-row: 2 / 5;
    grid-column: 2;
    cursor: col-resize;
    touch-action: none;
    border-left: 1px solid var(--rule);
    border-right: 1px solid var(--rule);
    background: var(--paper-sunk);
  }
  .g-grip:hover,
  .g-grip:focus-visible,
  .g-grip:active {
    background: color-mix(in srgb, var(--accent) 35%, var(--paper-sunk));
    outline: none;
  }
  .dropzone {
    grid-row: 3;
    grid-column: 1 / -1;
    margin: 8px;
    position: relative;
  }
  .main.split .dropzone { grid-row: 3 / 5; grid-column: 3; }
  @media (max-width: 720px) {
    /* a file is open on a phone: the message box shrinks to one line until it is used */
    .g-bottom.compact :global(.meta) { display: none; }
    .g-bottom.compact:focus-within :global(.meta) { display: flex; }
  }
  .dropzone {
    z-index: 20;
    display: grid;
    place-content: center;
    border: 2px dashed var(--accent);
    border-radius: 14px;
    background: color-mix(in srgb, var(--paper) 82%, transparent);
    color: var(--accent);
    font-weight: 650;
    pointer-events: none;
  }
  .bare {
    padding: 10px 12px;
  }
  .menu {
    display: none;
  }
  .empty {
    flex: 1;
    display: grid;
    align-content: center;
    justify-items: start;
    gap: 14px;
    padding: 0 24px;
    container-type: inline-size;
  }
  /* Sized from the space it actually has (cqw), so a narrow window shrinks the type instead of wrapping it. */
  /* On a desktop the block gets room to breathe next to the sidebar. Heading, text and button still share one left edge. */
  @media (min-width: 901px) {
    .empty {
      padding-inline: clamp(40px, 6vw, 112px);
    }
  }
  .empty h1 {
    margin: 0;
    font-size: clamp(24px, 9.5cqw, 64px);
    line-height: 1.1;
    letter-spacing: -0.04em;
    font-weight: 780;
    word-break: keep-all;
    text-wrap: balance;
  }
  .empty p {
    margin: 0 0 6px;
    color: var(--ink-2);
    max-width: 32rem;
  }
  .flash {
    position: fixed;
    left: 50%;
    top: calc(72px + env(safe-area-inset-top));
    translate: -50% 0;
    z-index: 120;
    padding: 8px 16px;
    border-radius: 999px;
    border: 1px solid var(--rule);
    background: var(--raised);
    color: var(--ink);
    font-size: 13.5px;
    font-weight: 560;
    box-shadow: var(--shadow);
    animation: flash-in 0.14s ease-out;
  }
  @keyframes flash-in {
    from {
      opacity: 0;
      translate: -50% -6px;
    }
  }
  .error {
    position: absolute;
    left: 50%;
    top: 12px;
    translate: -50% 0;
    z-index: 40;
    display: flex;
    gap: 8px;
    align-items: center;
    padding: 6px 6px 6px 12px;
    border-radius: 10px;
    border: 1px solid color-mix(in srgb, var(--bad) 55%, var(--rule));
    background: var(--raised);
    color: var(--ink);
    box-shadow: var(--shadow);
    max-width: calc(100% - 24px);
    font-size: 14px;
  }
  .error::before {
    content: '';
    flex: none;
    width: 8px;
    height: 8px;
    border-radius: 2px;
    background: var(--bad);
  }
  .error .icon-btn {
    color: var(--ink-3);
  }

  .insp.drawer {
    position: fixed;
    right: 0;
    top: 0;
    bottom: 0;
    width: min(320px, 88vw);
    z-index: 32;
    box-shadow: var(--shadow);
  }
  .insp.drawer.wide {
    width: min(440px, 94vw);
  }
  @media (max-width: 720px) {
    .insp.drawer,
    .insp.drawer.wide {
      width: 100%;
    }
  }
  .scrim.on {
    display: block;
    position: fixed;
    inset: 0;
    z-index: 31;
    background: var(--scrim);
  }
  @media (max-width: 1100px) {
    .shell.inspector {
      grid-template-columns: var(--sidebar-w) minmax(0, 1fr);
    }
  }
  @media (max-width: 900px) {
    .shell,
    .shell.inspector {
      grid-template-columns: minmax(0, 1fr);
    }
    .side {
      position: fixed;
      inset: 0 auto 0 0;
      width: 100%;
      z-index: 33;
      transform: translateX(-102%);
      visibility: hidden;
      transition: transform 0.2s ease-out, visibility 0s 0.2s;
    }
    [data-sidebar='open'] .side {
      transform: none;
      visibility: visible;
      transition: transform 0.2s ease-out;
    }
    .menu {
      display: inline-grid;
    }
  }
</style>
