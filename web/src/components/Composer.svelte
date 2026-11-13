<script lang="ts">
  import { trackPopover } from '../lib/popover.svelte';
  import { t } from '../lib/i18n.svelte';
  import { app } from '../lib/app.svelte';
  import { handleSlash, builtins } from '../lib/builtins';
  import type { Chat } from '../lib/chat.svelte';
  import { askPermission } from '../lib/notify';
  import { dismiss } from '../lib/actions';
  import { indentEdit } from '../lib/indent';
  import type { ImagePart } from '../lib/protocol';
  import { S } from '../lib/strings';
  import BgChips from './BgChips.svelte';
  import ComposerMeta from './ComposerMeta.svelte';
  import TodoTray from './TodoTray.svelte';
  import { FILE_MIME } from '../lib/editor.svelte';
  import Icon from './Icon.svelte';

  let { chat }: { chat: Chat } = $props();

  type Menu = 'plus' | 'perm' | 'model' | 'think' | 'ctx' | 'limit' | null;

  let text = $state('');
  let images = $state<ImagePart[]>([]);
  let area: HTMLTextAreaElement;
  let fileInput: HTMLInputElement;
  let pick = $state(0);
  let dragging = $state(false);
  let menu = $state<Menu>(null);
  // the phone's back button closes an open popover first (App.svelte)
  $effect(() => {
    if (menu) return trackPopover(() => (menu = null));
  });
  // A touch keyboard's Enter is the newline key (enterkeyhint=new-line on the textarea); sending is the button's job.
  const coarse = matchMedia('(pointer: coarse)').matches;
  const toggle = (m: Exclude<Menu, null>) => {
    menu = menu === m ? null : m;
  };

  const entries = $derived.by(() => {
    const seen = new Set<string>();
    const all = [
      ...builtins.map((b) => ({ name: b.name, description: b.description })),
      ...chat.commands.filter((c) => c.name !== 'oven-permission' && c.name !== 'oven-retry').map((c) => ({ name: c.name, description: c.description ?? '' })),
    ];
    return all.filter((c) => !seen.has(c.name) && seen.add(c.name));
  });
  const slash = $derived.by(() => {
    const m = text.match(/^\/([^\s]*)$/);
    if (!m) return [];
    const q = m[1].toLowerCase();
    return entries.filter((c) => c.name.toLowerCase().includes(q)).slice(0, 8);
  });
  $effect(() => { slash.length; pick = 0; });

  const ctxChip = $derived(chat.editors.context);
  const canSend = $derived((text.trim().length > 0 || images.length > 0) && chat.link === 'open');
  const queued = $derived(chat.queue.steering.length + chat.queue.followUp.length);
  // One line to start, growing with content up to 40% of the viewport.
  $effect(() => {
    text;
    if (!area) return;
    area.style.height = 'auto';
    area.style.height = Math.min(area.scrollHeight, window.innerHeight * 0.4) + 'px';
  });

  // Extensions and fork can put text back into the editor.
  $effect(() => {
    const p = chat.prefill;
    if (!p) return;
    text = p.text;
    chat.prefill = null;
    queueMicrotask(() => area?.focus());
  });

  function submit(mode: 'steer' | 'followUp' | 'now' = 'steer') {
    if (!canSend) return;
    if (mode === 'now' && !chat.running) mode = 'steer';
    askPermission();
    const msg = text.trim();
    // pi's terminal-only commands (/new, /model, ...) are mapped to RPC here instead of being sent as prompts.
    if (!images.length && handleSlash(chat, msg)) {
      text = '';
      return;
    }
    // what is selected in the open file goes along (once); a slash command never carries it
    const body = msg + (msg.startsWith('/') ? '' : chat.editors.takeContext());
    void (mode === 'now' ? chat.sendNow(body, images) : chat.send(body, images, mode));
    if (chat.conn.link === 'open') { text = ''; images = []; } // a dead link keeps the text: the send failed, the toast says so
  }

  function accept(i: number) {
    const c = slash[i];
    if (!c) return;
    text = `/${c.name} `;
    area.focus();
  }

  function onkeydown(e: KeyboardEvent) {
    // Korean/Japanese/Chinese IME: Enter while composing confirms the syllable, it must not send.
    if (e.isComposing || e.keyCode === 229) return;
    // Touch: Enter makes a line break; the send button sends. (Desktop keeps Enter-to-send.)
    if (coarse && e.key === 'Enter') return;

    if (slash.length) {
      if (e.key === 'ArrowDown') { e.preventDefault(); pick = (pick + 1) % slash.length; return; }
      if (e.key === 'ArrowUp') { e.preventDefault(); pick = (pick - 1 + slash.length) % slash.length; return; }
      if (e.key === 'Tab') { e.preventDefault(); accept(pick); return; }
      if (e.key === 'Enter' && !e.shiftKey) {
        e.preventDefault();
        // A command that takes no argument runs right away; otherwise complete it and let the person type more.
        const exact = slash[pick].name === text.slice(1).trim();
        exact ? submit() : accept(pick);
        return;
      }
    }
    // Tab is an indent, not a way to leave the box and not a send: Tab and Shift+Tab indent and outdent.
    if (e.key === 'Tab' && !e.ctrlKey && !e.altKey && !e.metaKey) {
      e.preventDefault();
      const r = indentEdit(text, area.selectionStart, area.selectionEnd, e.shiftKey);
      text = r.text;
      queueMicrotask(() => { area.setSelectionRange(r.start, r.end); });
      return;
    }
    // Only the Enter key sends (checked by its code too: some soft keyboards report other keys oddly).
    if (e.key === 'Enter' && e.keyCode !== 9 && !e.shiftKey) {
      e.preventDefault();
      submit(e.ctrlKey || e.metaKey ? 'now' : e.altKey ? 'followUp' : 'steer');
    } else if (e.key === 'Escape' && chat.running) {
      chat.abort();
    }
  }

  async function addFiles(files: Iterable<File>) {
    for (const f of files) {
      if (!f.type.startsWith('image/')) continue;
      const data = await new Promise<string>((res) => {
        const r = new FileReader();
        r.onload = () => res(String(r.result).split(',')[1] ?? '');
        r.readAsDataURL(f);
      });
      images.push({ type: 'image', data, mimeType: f.type });
    }
  }

  function onpaste(e: ClipboardEvent) {
    const files = [...(e.clipboardData?.files ?? [])];
    if (files.some((f) => f.type.startsWith('image/'))) {
      e.preventDefault();
      void addFiles(files);
    }
  }

  function ondrop(e: DragEvent) {
    e.preventDefault();
    dragging = false;
    // a file from the explorer becomes its path in the message, to refer to it
    const dragged = e.dataTransfer?.getData(FILE_MIME);
    if (dragged) {
      e.stopPropagation(); // the middle would open it; here it only names it
      try {
        const { path, runtime } = JSON.parse(dragged);
        if (runtime === chat.runtime.id) text += (text && !/\s$/.test(text) ? ' ' : '') + path + ' ';
      } catch { /* not ours */ }
      return;
    }
    void addFiles(e.dataTransfer?.files ?? []);
  }

</script>

<div class="dock">
  <BgChips {chat} />

  <!-- Messages waiting for the agent: a card of their own above the task list, so the list stays against the
       message box and does not move when the queue comes and goes. -->
  {#if queued}
    <section class="queue" aria-label={S.queue}>
      <span class="qhead"><Icon name="clock" size={13} />{S.queue} {queued}</span>
      <ul>
        {#each chat.queue.steering as q}<li><b>{S.steer}</b><span class="q">{q}</span></li>{/each}
        {#each chat.queue.followUp as q}<li><b>{S.followUp}</b><span class="q">{q}</span></li>{/each}
      </ul>
      {#if chat.running}
        <button class="now" type="button" title={S.sendNowHint} onclick={() => void chat.sendNow('')}><Icon name="now" size={14} />{S.sendNow}</button>
      {/if}
    </section>
  {/if}

  <!-- the model's task list sits right on the message box -->
  <TodoTray {chat} />

  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="composer"
    class:hot={chat.running}
    class:dragging
    ondragover={(e) => { e.preventDefault(); dragging = true; }}
    ondragleave={() => (dragging = false)}
    {ondrop}
  >
    {#if slash.length}
      <ul class="slash" role="listbox" aria-label={S.slashCommands}>
        {#each slash as c, i (c.name)}
          <li role="option" aria-selected={i === pick}>
            <button type="button" class:on={i === pick} onmousedown={(e) => { e.preventDefault(); accept(i); }}>
              <span class="n mono">/{c.name}</span><span class="d">{c.description}</span>
            </button>
          </li>
        {/each}
      </ul>
    {/if}

    {#if ctxChip}
      <div class="ctxrow">
        <span class="ctx" class:auto={ctxChip.auto}>
          <button class="main" type="button" aria-pressed={ctxChip.auto} title={ctxChip.auto ? t('composer.ctxAuto') : t('composer.ctxFile')}
            onclick={() => { if (!ctxChip.auto) chat.editors.attachFile = true; }}>
            <Icon name="file" size={13} /><span class="mono" data-priv="files">{ctxChip.label}</span>{#if !ctxChip.auto}<span class="hint">{t('composer.attach')}</span>{/if}
          </button>
          {#if ctxChip.auto}
            <button class="x" type="button" aria-label={t('composer.detach')} onclick={() => { if (chat.editors.selectionKey && chat.editors.dismissed !== chat.editors.selectionKey && chat.editors.selection?.text) chat.editors.dismissed = chat.editors.selectionKey; else chat.editors.attachFile = false; }}>×</button>
          {/if}
        </span>
      </div>
    {/if}

    {#if images.length}
      <div class="thumbs">
        {#each images as img, i}
          <div class="th">
            <img alt="" src={`data:${img.mimeType};base64,${img.data}`} />
            <button type="button" aria-label={t('composer.removeImage')} onclick={() => images.splice(i, 1)}><Icon name="x" size={12} /></button>
          </div>
        {/each}
      </div>
    {/if}

    <div class="line">
      <div class="pop" use:dismiss={() => { if (menu === 'plus') menu = null; }}>
        <button class="round" type="button" aria-label={t('composer.attachMenu')} aria-haspopup="menu" aria-expanded={menu === 'plus'} onclick={() => toggle('plus')}>
          <Icon name="plus" />
        </button>
        {#if menu === 'plus'}
          <div class="sheet narrow" role="menu">
            <button role="menuitem" type="button" onclick={() => ((menu = null), fileInput.click())}><Icon name="clip" size={16} />{t('composer.attachImage')}</button>
          </div>
        {/if}
      </div>
      <input bind:this={fileInput} type="file" accept="image/*" multiple hidden onchange={(e) => { void addFiles(e.currentTarget.files ?? []); e.currentTarget.value = ''; }} />

      <textarea
        bind:this={area}
        bind:value={text}
        {onkeydown}
        {onpaste}
        rows="1"
        placeholder={chat.running ? (app.narrow ? S.composerPlaceholderRunningShort : S.composerPlaceholderRunning) : S.composerPlaceholder}
        aria-label={t('composer.message')}
        spellcheck="false"
        enterkeyhint={'new-line' as never} /* spec value predates the tooling's enum */
      ></textarea>

      {#if chat.running && !canSend}
        <button class="round solid" type="button" aria-label={S.stop} title="{S.stop} (Esc)" onclick={() => chat.abort()}><Icon name="stop" size={16} /></button>
      {:else}
        {#if chat.running}
          <button class="round" type="button" aria-label={S.sendNow} title="{S.sendNow}: {S.sendNowHint} (Ctrl+Enter)" onclick={() => submit('now')}><Icon name="now" size={17} /></button>
        {/if}
        <button class="round solid" type="button" disabled={!canSend} aria-label={chat.running ? S.steer : S.send} title={chat.running ? `${S.steer} (Enter), ${S.followUp} (Alt+Enter)` : S.send} onclick={() => submit()}>
          <Icon name="send" size={16} />
        </button>
      {/if}
    </div>
  </div>
  <ComposerMeta {chat} bind:menu {toggle} />
</div>

<style>
  .dock {
    padding: 0 24px 12px;
  }
  .queue {
    max-width: calc(var(--measure) + 74px);
    margin: 0 auto 8px;
    padding: 8px 8px 8px 14px;
    border: 1px solid var(--rule);
    border-radius: 10px;
    background: var(--raised);
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    grid-template-areas: 'head now' 'list list';
    align-items: center;
    row-gap: 4px;
    font-size: 13px;
  }
  .qhead {
    grid-area: head;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    color: var(--ink-3);
    font-size: 12px;
    font-weight: 600;
  }
  .queue ul {
    grid-area: list;
    list-style: none;
    margin: 0;
    padding: 0 6px 0 0;
    display: grid;
    gap: 2px;
    /* four whole lines, then it scrolls (never a line cut in half) */
    max-height: calc(4 * 20px + 3 * 2px);
    overflow-y: auto;
    scrollbar-width: none;
  }
  .queue ul::-webkit-scrollbar {
    display: none;
  }
  .queue li {
    display: flex;
    gap: 8px;
    line-height: 20px;
    min-width: 0;
    color: var(--ink);
  }
  .queue b {
    flex: none;
    color: var(--ink-3);
    font-weight: 560;
  }
  .queue .q {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .queue .now {
    grid-area: now;
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 26px;
    padding: 0 10px;
    border-radius: 7px;
    font-size: 12.5px;
    font-weight: 600;
    color: var(--accent);
  }
  @media (hover: hover) {
    .queue .now:hover {
      background: var(--accent-wash);
    }
  }
  .composer {
    max-width: calc(var(--measure) + 74px);
    margin-inline: auto;
  }
  .composer {
    position: relative;
    background: var(--raised);
    border: 1px solid var(--rule);
    border-radius: 16px;
    box-shadow: var(--shadow);
    transition: border-color 0.2s, box-shadow 0.4s;
  }
  .composer:focus-within {
    border-color: var(--ink-3);
  }
  .ctxrow {
    padding: 8px 12px 0;
  }
  .ctx {
    display: inline-flex;
    align-items: center;
    max-width: 100%;
    border-radius: 999px;
    border: 1px dashed var(--rule);
    color: var(--ink-3);
    font-size: 12.5px;
  }
  .ctx.auto {
    border-style: solid;
    border-color: color-mix(in srgb, var(--accent) 55%, var(--rule));
    color: var(--ink);
    background: var(--accent-wash);
  }
  .ctx .main {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    height: 26px;
    padding: 0 10px;
    border-radius: 999px;
  }
  .ctx .main .mono {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    direction: rtl;
    text-align: left;
  }
  .ctx .hint {
    flex: none;
    color: var(--ink-3);
  }
  .ctx .x {
    flex: none;
    width: 22px;
    height: 22px;
    margin-right: 3px;
    border-radius: 50%;
    color: var(--ink-3);
    font-size: 15px;
    line-height: 1;
  }
  .composer.dragging {
    border-color: var(--accent);
  }
  .line {
    display: flex;
    align-items: flex-end;
    gap: 6px;
    padding: 7px 8px;
  }
  textarea {
    flex: 1;
    min-width: 0;
    resize: none;
    border: 0;
    background: transparent;
    padding: 7px 4px;
    line-height: 1.5;
    min-height: 34px;
    outline: none;
  }
  textarea::placeholder {
    color: var(--ink-3);
  }
  .round {
    flex: none;
    width: 34px;
    height: 34px;
    display: grid;
    place-items: center;
    border-radius: 50%;
    color: var(--ink-2);
  }
  @media (hover: hover) {
    .round:hover:not(:disabled) {
      background: var(--paper-sunk);
      color: var(--ink);
    }
  }
  .round.solid {
    background: var(--accent);
    color: var(--accent-ink);
  }
  @media (hover: hover) {
    .round.solid:hover:not(:disabled) {
      background: var(--accent);
      filter: brightness(1.1);
    }
  }
  .round.solid:disabled {
    background: var(--paper-sunk);
    color: var(--ink-3);
    cursor: default;
  }
  .thumbs {
    display: flex;
    gap: 8px;
    padding: 10px 12px 0;
    flex-wrap: wrap;
  }
  .th {
    position: relative;
  }
  .th img {
    height: 52px;
    border-radius: 6px;
    border: 1px solid var(--rule);
    display: block;
  }
  .th button {
    position: absolute;
    top: -6px;
    right: -6px;
    width: 20px;
    height: 20px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    background: var(--raised);
    border: 1px solid var(--rule);
    color: var(--ink-2);
    box-shadow: var(--shadow);
  }
  .slash {
    position: absolute;
    bottom: calc(100% + 6px);
    left: 0;
    right: 0;
    list-style: none;
    margin: 0;
    padding: 4px;
    background: var(--raised);
    border: 1px solid var(--rule);
    border-radius: 12px;
    box-shadow: var(--shadow);
    max-height: 300px;
    overflow: auto;
    z-index: 5;
  }
  .slash button {
    width: 100%;
    display: grid;
    grid-template-columns: 11rem minmax(0, 1fr);
    gap: 12px;
    align-items: baseline;
    text-align: left;
    padding: 7px 10px;
    border-radius: 8px;
  }
  .slash button.on {
    background: var(--accent-wash);
  }
  .slash .n {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .slash .d {
    color: var(--ink-3);
    font-size: 13px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  @media (max-width: 720px) {
    .dock {
      padding: 0 14px calc(10px + env(safe-area-inset-bottom));
    }
    .slash button {
      grid-template-columns: 1fr;
      gap: 0;
    }
    /* Inside the box the space left of the text (the + button) and right of it (the send button) is the same, and tighter than on a wide screen. */
    .line {
      padding: 7px;
      gap: 0;
    }
    /* The solid send button sits the same distance from the edge on every side (7px, as above and below it), and the box's
       corners are concentric with it (7px + the button's 16px radius), so the button doesn't crowd the curve. */
    .composer {
      border-radius: 23px;
    }
    .round {
      width: 32px;
      height: 32px;
      margin-bottom: 1px; /* centred on the first line of text */
    }
    textarea {
      padding: 7px 6px;
    }
  }
</style>
