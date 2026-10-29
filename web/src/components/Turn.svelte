<script lang="ts">
  import WaitBubble from './WaitBubble.svelte';
  import type { Block, Chat, Item, ToolBlock } from '../lib/chat.svelte';
  import { app } from '../lib/app.svelte';
  import { showcase } from '../lib/showcase.svelte';
  import { contextMenu, type MenuItem } from '../lib/contextmenu.svelte';
  import { S } from '../lib/strings';
  import { messageKey } from '../lib/pins.svelte';
  import { formatMs, formatRate, type Speed } from '../lib/speed';
  import { clock, compactNumber, duration } from '../lib/util';
  import Icon from './Icon.svelte';
  import Markdown from './Markdown.svelte';
  import ToolRow from './ToolRow.svelte';
  import Thinking from './Thinking.svelte';

  let { item, chat, userIndex, turn }: { item: Item; chat: Chat; userIndex: number; turn: { user: number; end: boolean } } = $props();

  type Group = { kind: 'text'; text: string; last: boolean } | { kind: 'thinking'; text: string; redacted?: boolean; last: boolean } | { kind: 'tools'; tools: ToolBlock[] };

  const groups = $derived.by<Group[]>(() => {
    if (item.kind !== 'assistant') return [];
    const out: Group[] = [];
    item.blocks.forEach((b: Block | undefined, i) => {
      if (!b) return;
      const last = i === item.blocks.length - 1;
      if (b.type === 'tool') {
        const prev = out[out.length - 1];
        if (prev?.kind === 'tools') prev.tools.push(b);
        else out.push({ kind: 'tools', tools: [b] });
      } else if (b.type === 'text') {
        if (b.text) out.push({ kind: 'text', text: b.text, last });
      } else out.push({ kind: 'thinking', text: b.text, redacted: b.redacted, last });
    });
    return out;
  });

  function turnItems(e: { target: EventTarget | null }): MenuItem[] {
    const t = e.target instanceof Element ? e.target : null;
    const out: MenuItem[] = [];

    const sel = getSelection();
    const picked = sel?.toString().trim() ?? '';
    if (picked && t?.closest('article')?.contains(sel!.anchorNode)) out.push({ label: '선택한 텍스트 복사', icon: 'copy', run: () => app.copy(picked) });

    const link = t?.closest('a');
    if (link) {
      out.push({ label: '링크 주소 복사', icon: 'link', run: () => app.copy(link.href) });
      out.push({ label: '링크를 새 탭에서 열기', icon: 'external', run: () => window.open(link.href, '_blank', 'noopener') });
    }
    const code = t?.closest('.code')?.querySelector('code')?.textContent;
    if (code) out.push({ label: '코드 복사', icon: 'code', run: () => app.copy(code) });
    if (out.length) out.push('sep');

    if (item.kind === 'user') {
      out.push({ label: '메시지 복사', icon: 'copy', run: () => app.copy(item.kind === 'user' ? item.text : '') });
      out.push({ label: pinned ? '핀에서 빼기' : '핀에 추가', icon: 'pin', run: togglePin });
      out.push({ label: '입력창에 넣기', icon: 'reply', run: () => (chat.prefill = { text: item.kind === 'user' ? item.text : '', n: Date.now() }) });
      if (userIndex >= 0) {
        out.push({ label: S.fork, icon: 'fork', disabled: chat.running, run: () => chat.fork(userIndex) });
        out.push({ label: '이 메시지로 다시 생성', icon: 'refresh', disabled: chat.running || hasImages, run: () => void regenerate(userIndex) });
        out.push('sep');
        out.push({ label: '여기부터 지우기', icon: 'trash', danger: true, disabled: chat.running, run: () => void rewind(userIndex) });
      }
    } else if (item.kind === 'assistant') {
      const text = item.blocks.flatMap((b) => (b?.type === 'text' ? [b.text] : [])).join('\n\n');
      out.push({ label: '답변 복사', icon: 'copy', disabled: !text, run: () => app.copy(text) });
      if (showBar) out.push({ label: pinned ? '핀에서 빼기' : '핀에 추가', icon: 'pin', disabled: !text, run: togglePin });
      if (turn.user >= 0) out.push({ label: '다시 생성', icon: 'refresh', disabled: chat.running || item.live, run: () => void regenerate(turn.user) });
    } else if (item.kind === 'task') {
      const t = item;
      out.push({ label: '출력 보기', icon: 'terminal', run: () => { chat.bgFocus = t.id; chat.bgOpen = true; } });
      out.push({ label: '명령 복사', icon: 'copy', disabled: !t.command, run: () => app.copy(t.command) });
    } else if (item.kind === 'bash') {
      out.push({ label: '명령 복사', icon: 'terminal', run: () => app.copy(item.kind === 'bash' ? item.command : '') });
      out.push({ label: '출력 복사', icon: 'copy', disabled: !item.output, run: () => app.copy(item.kind === 'bash' ? item.output : '') });
    } else {
      out.push({ label: '복사', icon: 'copy', run: () => app.copy(item.kind === 'summary' || item.kind === 'notice' ? item.text : '') });
    }
    return out;
  }

  // ── actions under a block ───────────────────────────────────────────────
  const userText = $derived(item.kind === 'user' ? item.text : '');
  const answerText = $derived(item.kind === 'assistant' ? item.blocks.flatMap((b) => (b?.type === 'text' ? [b.text] : [])).join('\n\n') : '');
  const userItem = $derived(item.kind === 'user' ? item : null);
  const hasImages = $derived(userItem ? userItem.images.length > 0 : false);

  /** Go back to before this message, and carry on from there in a new session; the old one stays. */
  async function rewind(idx: number) {
    const ok = await app.ask({
      title: '여기부터 지울까요?',
      body: '이 메시지와 이후의 대화를 뺀 새 세션으로 이어집니다. 원래 세션의 기록은 그대로 남아 있어요.',
      confirm: '지우기',
      danger: true,
    });
    if (ok) await chat.fork(idx, 'rewind');
  }

  const regenerate = (idx: number) => chat.fork(idx, 'regenerate');

  /** "완료 · 3.2s", "실패 (종료 코드 2) · 1.0s" */
  function taskLine(t: Extract<Item, { kind: 'task' }>) {
    const word = t.status === 'completed' ? '완료' : t.status === 'killed' ? '중단됨' : '실패';
    const code = t.exitCode != null && t.exitCode !== 0 ? ` (종료 코드 ${t.exitCode})` : '';
    return [word + code, t.durationMs != null ? duration(t.durationMs) : ''].filter(Boolean).join(' · ');
  }

  /** Whether this block has its actions underneath (they take a row of their own, so nothing overlaps). */
  const showBar = $derived(
    (item.kind === 'user' && userIndex >= 0) ||
      (item.kind === 'assistant' && turn.end && !item.live && turn.user >= 0 && (answerText !== '' || !!item.error)),
  );

  /** The text a pin of this block would hold, and whether it is pinned now. */
  const pinText = $derived(item.kind === 'user' ? item.text : item.kind === 'assistant' ? answerText : '');
  const pinRole = $derived<'user' | 'assistant'>(item.kind === 'user' ? 'user' : 'assistant');
  const mkey = $derived(showBar && pinText ? messageKey(pinRole, pinText, item.ts) : '');
  const pinned = $derived(!!mkey && chat.pins.hasMessage(mkey));
  function togglePin() {
    if (!pinText) return;
    app.flash(chat.pins.toggleMessage(pinRole, pinText, item.ts) ? '핀에 추가했습니다' : '핀에서 뺐습니다');
  }

  // How fast the answer is coming. While it streams, the rate the llama-infra package measured is used if it has
  // spoken since this answer began; otherwise it is estimated here, and marked with a ~.
  const liveSpeed = $derived.by<Speed | null>(() => {
    if (item.kind !== 'assistant' || !item.live || !item.speed || item.speed.ttft === undefined) return null;
    const p = chat.pkgSpeed;
    if (p?.gen && item.speed.first !== undefined && p.at >= item.speed.first) return { ...item.speed, tps: p.gen, est: false };
    return item.speed;
  });
  const speedText = (s: Speed | undefined, ttft = true) =>
    s && s.tps !== undefined ? `${s.est ? '~' : ''}${formatRate(s.tps)} tok/s${ttft && s.ttft !== undefined ? ` · TTFT ${formatMs(s.ttft)}` : ''}` : s?.ttft !== undefined && ttft ? `TTFT ${formatMs(s.ttft)}` : '';

  const waiting = $derived(item.kind === 'assistant' && item.live && item.blocks.length === 0);
</script>

<article class="turn {item.kind}" class:has-bar={showBar} data-msg={mkey || undefined} use:contextMenu={turnItems}>
  <div class="margin">
    <time>{clock(item.ts)}</time>
    {#if item.kind === 'assistant' && item.usage && item.usage.output > 0}
      <span class="tok" title="출력 토큰 {item.usage.output.toLocaleString()} (입력 {item.usage.input.toLocaleString()})">{compactNumber(item.usage.output)}</span>
    {/if}
  </div>

  <div class="body" data-priv={item.kind === 'assistant' ? 'chat' : undefined}>
    {#if item.kind === 'user'}
      <div class="said">
        {#if item.skills.length}
          <div class="skills">{#each item.skills as s}<span>{s}</span>{/each}</div>
        {/if}
        {#if item.text}<div class="text" data-priv="chat">{showcase.text(item.text)}</div>{/if}
        {#if item.images.length}
          <div class="thumbs">
            {#each item.images as img}<img alt="" src={`data:${img.mimeType};base64,${img.data}`} />{/each}
          </div>
        {/if}
      </div>
      {#if userIndex >= 0}
        <div class="acts" role="group" aria-label="메시지 동작">
          <div class="bar">
            <button type="button" title="메시지 복사" aria-label="메시지 복사" onclick={() => app.copy(userText)}><Icon name="copy" size={15} /></button>
            <button type="button" title="입력창에 넣기" aria-label="입력창에 넣기" onclick={() => (chat.prefill = { text: userText, n: Date.now() })}><Icon name="reply" size={15} /></button>
            <button type="button" title={S.fork} aria-label={S.fork} disabled={chat.running} onclick={() => chat.fork(userIndex)}><Icon name="fork" size={15} /></button>
            <button type="button" title={pinned ? '핀에서 빼기' : '핀에 추가'} aria-label={pinned ? '핀에서 빼기' : '핀에 추가'} aria-pressed={pinned} class="pin" onclick={togglePin}><Icon name="pin" size={15} /></button>
            <button type="button" title={hasImages ? '이미지가 있는 메시지는 다시 생성할 수 없습니다' : '이 메시지로 다시 생성'} aria-label="이 메시지로 다시 생성" disabled={chat.running || hasImages} onclick={() => regenerate(userIndex)}><Icon name="refresh" size={15} /></button>
            <button class="danger" type="button" title="여기부터 지우기" aria-label="여기부터 지우기" disabled={chat.running} onclick={() => rewind(userIndex)}><Icon name="trash" size={15} /></button>
            {#if item.ts}<time class="when">{clock(item.ts)}</time>{/if}
          </div>
        </div>
      {/if}
    {:else if item.kind === 'assistant'}
      {#each groups as g, gi (gi)}
        {#if g.kind === 'text'}
          <Markdown source={showcase.text(g.text)} streaming={item.live && g.last} />
        {:else if g.kind === 'thinking'}
          <Thinking text={g.text} redacted={g.redacted} live={item.live && g.last} />
        {:else}
          <div class="ledger">
            {#each g.tools as t (t.id)}<ToolRow tool={t} />{/each}
          </div>
        {/if}
      {/each}
      {#if waiting}<WaitBubble {chat} />{/if}
      {#if liveSpeed}<div class="speed mono" aria-hidden="true">{speedText(liveSpeed)}</div>{/if}
      {#if item.error}<p class="err">{item.error}</p>{/if}
      {#if item.stop === 'aborted'}<p class="faint">{S.aborted}</p>{/if}
      {#if showBar}
        <div class="acts" role="group" aria-label="답변 동작">
          <div class="bar">
            <button type="button" title="답변 복사" aria-label="답변 복사" disabled={!answerText} onclick={() => app.copy(answerText)}><Icon name="copy" size={15} /></button>
            <button type="button" title="다시 생성" aria-label="다시 생성" disabled={chat.running} onclick={() => regenerate(turn.user)}><Icon name="refresh" size={15} /></button>
            <button type="button" title={pinned ? '핀에서 빼기' : '핀에 추가'} aria-label={pinned ? '핀에서 빼기' : '핀에 추가'} aria-pressed={pinned} class="pin" disabled={!answerText} onclick={togglePin}><Icon name="pin" size={15} /></button>
            {#if item.ts}<time class="when">{clock(item.ts)}{#if item.usage && item.usage.output > 0} · {compactNumber(item.usage.output)}{/if}{#if speedText(item.speed)} · {speedText(item.speed)}{/if}</time>{/if}
          </div>
        </div>
      {/if}
    {:else if item.kind === 'bash'}
      <div class="ledger">
        <div class="bashrow">
          <span class="mono"><span class="prompt">$ </span>{showcase.text(item.command)}</span>
          {#if item.exitCode != null && item.exitCode !== 0}<span class="exit">exit {item.exitCode}</span>{/if}
        </div>
        {#if item.output}<pre class="mono bashout">{showcase.text(item.output)}</pre>{/if}
      </div>
    {:else if item.kind === 'task'}
      <div class="tnotice" class:hold={chat.bgHandoffHold.includes(item.id)} class:bad={item.status !== 'completed'} data-task-notice={item.id}>
        <span class="tdot" aria-hidden="true"></span>
        <span class="tnm" data-priv="titles">{showcase.text(item.name)}</span>
        <span class="tres">{taskLine(item)}</span>
        <button class="tout" type="button" onclick={() => { chat.bgFocus = item.id; chat.bgOpen = true; }}>출력 보기</button>
      </div>
    {:else if item.kind === 'summary'}
      <details class="sum">
        <summary>{item.label}</summary>
        <p>{showcase.text(item.text)}</p>
      </details>
    {:else}
      <p class="notice {item.level}">{showcase.text(item.text)}</p>
    {/if}
  </div>
</article>

<style>
  /* The text runs the full width of the message box. The time and token count do not take a column out of it:
     they hang in the space to its left when the window has that space, and move into the hover bar when not. */
  .turn {
    position: relative;
    padding: 14px 0;
  }
  .margin {
    display: none;
    position: absolute;
    right: calc(100% + 18px);
    top: 16px;
    width: 56px;
    text-align: right;
    font-size: 12px;
    line-height: 1.6;
    color: var(--ink-3);
    align-content: start;
  }
  .when {
    align-self: center;
    margin-left: 8px;
    font-size: 11.5px;
    color: var(--ink-3);
    white-space: nowrap;
  }
  @container chat (min-width: 960px) {
    .margin { display: grid; }
    .when { display: none; }
  }
  .tok {
    opacity: 0.75;
  }
  .body {
    min-width: 0;
  }
  .user .said {
    position: relative;
    border-left: 2px solid var(--accent);
    padding: 1px 0 1px 16px;
  }
  .text {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    font-weight: 520;
  }
  .skills {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
    margin-bottom: 6px;
  }
  .skills span {
    font-size: 12px;
    padding: 1px 8px;
    border-radius: 999px;
    background: var(--accent-wash);
    color: var(--accent);
  }
  .thumbs {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
    margin-top: 8px;
  }
  .thumbs img {
    max-height: 140px;
    border-radius: 6px;
    border: 1px solid var(--rule);
  }
  /* Actions sit just under the block, in a row of their own so they never touch what comes next. Icons only. */
  .speed {
    margin-top: 6px;
    font-size: 12px;
    color: var(--ink-3);
    font-variant-numeric: tabular-nums;
  }
  .turn.has-bar {
    padding-bottom: 0;
  }
  .acts {
    position: relative;
    height: 30px;
  }
  .bar {
    position: absolute;
    top: 4px;
    left: 14px;
    align-items: center;
    z-index: 2;
    display: flex;
    gap: 1px;
    opacity: 0;
    pointer-events: none;
    transition: opacity 0.12s;
  }
  .assistant .bar {
    left: -6px;
  }
  .bar button {
    width: 28px;
    height: 26px;
    display: inline-grid;
    place-items: center;
    border-radius: 7px;
    color: var(--ink-3);
  }
  .bar button.pin[aria-pressed='true'] {
    color: var(--accent);
  }
  .bar button.danger:not(:disabled) {
    color: color-mix(in srgb, var(--bad) 75%, var(--ink-3));
  }
  .bar button:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .bar:focus-within {
    opacity: 1;
    pointer-events: auto;
  }
  @media (hover: hover) {
    .turn:hover .bar {
      opacity: 1;
      pointer-events: auto;
    }
    .bar button:not(:disabled):hover {
      background: var(--paper-sunk);
      color: var(--ink);
    }
    .bar button.danger:not(:disabled):hover {
      color: var(--bad);
    }
  }
  /* no hover on a touch screen: they are there, quietly */
  @media (hover: none) {
    .bar {
      opacity: 0.7;
      pointer-events: auto;
    }
  }

  /* a background task ended: pi's notice, as a bubble */
  .tnotice {
    display: inline-flex;
    align-items: center;
    gap: 9px;
    max-width: 100%;
    padding: 6px 8px 6px 12px;
    border: 1px solid var(--rule);
    border-radius: 999px;
    background: var(--raised);
    font-size: 13.5px;
  }
  .tnotice.hold {
    visibility: hidden;
  }
  .tdot {
    flex: none;
    width: 8px;
    height: 8px;
    border-radius: 2px;
    background: var(--ok);
  }
  .tnotice.bad .tdot {
    background: var(--bad);
  }
  .tnotice.bad {
    border-color: color-mix(in srgb, var(--bad) 45%, var(--rule));
  }
  .tnm {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 620;
  }
  .tres {
    flex: none;
    color: var(--ink-2);
    white-space: nowrap;
  }
  .tout {
    flex: none;
    height: 24px;
    padding: 0 10px;
    border-radius: 999px;
    background: var(--paper-sunk);
    color: var(--ink-2);
    font-size: 12.5px;
  }
  @media (hover: hover) {
    .tout:hover {
      background: var(--rule);
      color: var(--ink);
    }
  }

  .ledger {
    margin: 4px 0 10px;
    border-top: 1px solid var(--rule);
    border-bottom: 1px solid var(--rule);
  }
  .ledger:first-child {
    margin-top: 0;
  }
  .sum {
    margin: 0 0 10px;
    color: var(--ink-2);
    font-size: 14px;
  }
  .sum summary {
    cursor: pointer;
    color: var(--ink-3);
    font-weight: 560;
  }
  .sum p {
    margin: 6px 0 0;
    padding-left: 14px;
    border-left: 2px solid var(--rule);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    max-height: 340px;
    overflow: auto;
  }
  .err {
    color: var(--bad);
    margin: 4px 0 0;
  }
  .faint {
    color: var(--ink-3);
    margin: 4px 0 0;
    font-size: 13px;
  }
  .bashrow {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    padding: 7px 4px;
    font-size: 12.5px;
  }
  .prompt {
    color: var(--accent);
  }
  .exit {
    color: var(--bad);
    font-size: 12px;
  }
  .bashout {
    margin: 0 0 8px;
    padding: 10px 12px;
    background: var(--code-bg);
    border-radius: 6px;
    max-height: 360px;
    overflow: auto;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    font-size: 12.5px;
  }
  .notice {
    margin: 0;
    font-size: 13.5px;
    color: var(--ink-2);
  }
  .notice.error {
    color: var(--bad);
  }
  .notice.warn {
    color: var(--accent);
  }

</style>
