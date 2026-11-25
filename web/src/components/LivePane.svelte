<script lang="ts">
  import { t } from '../lib/i18n.svelte';
  import { untrack } from 'svelte';
  import { api, HttpError } from '../lib/api';
  import { app } from '../lib/app.svelte';
  import type { Chat } from '../lib/chat.svelte';
  import { addedLines, countLines, draftCounts, editLines, gitLines, piLines, type Line } from '../lib/composing';
  import { isImage } from '../lib/editor.svelte';
  import { highlight } from '../lib/markdown';
  import { showcase } from '../lib/showcase.svelte';
  import { baseName } from '../lib/util';
  import FileIcon from './FileIcon.svelte';
  import Icon from './Icon.svelte';

  // The newest change to a file, as it happens: what the model is composing (not on the disk yet), then what the tool
  // saved (pi's diff), or for a change nobody told of, what the disk holds.
  let { chat }: { chat: Chat } = $props();
  const live = $derived(chat.live);

  const MAX = 400;
  const MAX_TOUCHES = 12;

  const path = $derived(live.view);
  const draft = $derived(path ? live.draftOf(path) : undefined);
  const failed = $derived(path && !draft && live.failed?.key === path ? live.failed : null);
  const disk = $derived(path && !draft && !failed ? live.disk[path] : undefined);
  const change = $derived(path ? live.changes.get(path) : undefined);
  const whole = $derived(!!draft && draft.phase !== 'composing');

  interface Part { head?: string; lines: Line[]; above?: number; below?: number }

  // ── what the disk holds (for changes pi's tools did not make) ───────────
  let read = $state<{ key: string; parts: Part[]; note?: string } | null>(null);
  $effect(() => {
    const p = path;
    const d = disk;
    if (!p || !d) return;
    const key = `${p}\n${d.n}`;
    let gone = false;
    void (async () => {
      let out: { key: string; parts: Part[]; note?: string };
      try {
        const diff = d.k === 'mod' ? gitLines(await api.gitDiff(chat.runtime.id, p).catch(() => '')) : [];
        if (diff.length) out = { key, parts: [{ lines: diff.slice(0, MAX), below: Math.max(0, diff.length - MAX) }] };
        else if (isImage(p)) out = { key, parts: [], note: t('live.noPreview') };
        else {
          const f = await api.readFile(chat.runtime.id, p);
          out = { key, parts: [top(f.content, d.k === 'new' ? 'add' : 'ctx')] };
        }
      } catch (e) {
        out = { key, parts: [], note: e instanceof HttpError && e.status === 404 ? t('live.gone') : t('live.noPreview') };
      }
      if (!gone) untrack(() => (read = out));
    })();
    return () => (gone = true);
  });

  function top(text: string, k: Line['k']): Part {
    const all = text.split('\n');
    if (all[all.length - 1] === '') all.pop();
    return { lines: all.slice(0, MAX).map((x, i) => ({ k, n: i + 1, x })), below: Math.max(0, all.length - MAX) };
  }

  const parts = $derived.by<Part[]>(() => {
    const d = draft?.draft ?? failed?.draft;
    if (d) {
      if (d.kind === 'edit') return d.edits.map((e, i) => ({ head: d.edits.length > 1 ? t('live.nth', { n: i + 1 }) : undefined, lines: editLines(e, whole || !!failed || i < d.edits.length - 1) }));
      if (failed) return [top(d.text, 'add')];
      const a = addedLines(d.text, whole ? MAX : MAX / 2);
      return d.text ? [{ above: a.from, lines: d.append ? a.lines.map(({ k, x }) => ({ k, x })) : a.lines }] : [];
    }
    if (disk) return read?.key === `${path}\n${disk.n}` || read?.key.startsWith(`${path}\n`) ? read!.parts : [];
    const list = change?.touches ?? [];
    const from = Math.max(0, list.length - MAX_TOUCHES);
    return list.slice(from).map((tc, i) => {
      const head = list.length > 1 ? t('live.edit', { n: from + i + 1 }) : undefined;
      if (tc.kind === 'write') return { head, ...top(tc.content ?? '', 'add') };
      const l = tc.diff ? piLines(tc.diff) : [];
      return { head, lines: l.slice(0, MAX), below: Math.max(0, l.length - MAX) };
    });
  });
  const earlier = $derived(draft || failed || disk ? 0 : Math.max(0, (change?.touches.length ?? 0) - MAX_TOUCHES));
  const note = $derived(
    draft ? (parts.some((p) => p.lines.length) ? (draft.draft.vars ? t('live.shell') : '') : t('live.nothingYet'))
      : disk ? (read?.key.startsWith(`${path}\n`) ? read.note ?? '' : '')
        : !failed && change && !parts.some((p) => p.lines.length) ? t('live.noDiff') : '',
  );

  const counts = $derived.by(() => {
    if (failed) return null;
    if (draft) return draftCounts(draft.draft, whole);
    if (disk) {
      if (!read?.key.startsWith(`${path}\n`)) return null;
      const c = { added: 0, removed: 0 };
      for (const p of read.parts) { for (const l of p.lines) { if (l.k === 'add') c.added++; else if (l.k === 'del') c.removed++; } if (p.lines[0]?.k === 'add' && live.disk[path!]?.k === 'new') c.added += p.below ?? 0; }
      return c;
    }
    return change ? { added: change.added, removed: change.removed } : null;
  });
  const word = $derived(
    failed ? (failed.declined ? t('live.declined') : t('live.failed'))
      : draft ? (draft.phase === 'running' && draft.draft.shell ? t('live.running') : draft.phase === 'composing' ? t('live.composing') : draft.phase === 'ask' ? t('live.ask') : t('live.waiting'))
        : path && live.fresh[path] ? t('live.saved') : '',
  );
  const mark = $derived(path && (change || disk || live.marks[path]) ? live.kind(path) : '');

  // ── colours, remembered by line: while a file is being written only its last line changes ──
  const lang = $derived(path ? path.slice(path.lastIndexOf('.') + 1) : '');
  const painted = new Map<string, string>();
  function paint(x: string): string {
    const l = lang;
    const key = `${l}\n${x}`;
    let h = painted.get(key);
    if (h === undefined) {
      if (painted.size > 3000) painted.clear();
      // a very long line (minified) is left plain
      h = x.length > 1500 ? highlight(x) : highlight(x, l);
      painted.set(key, h);
    }
    return h;
  }

  // ── staying at the end of what is being written ─────────────────────────
  let code = $state<HTMLElement>();
  let stuck = true;
  let shown = '';
  $effect(() => {
    parts;
    const key = `${path}|${draft ? 'd' : failed ? 'f' : disk ? 'k' : 't' + (change?.touches.length ?? 0)}`;
    const writing = !!draft;
    const el = code;
    if (!el) return;
    const fresh = key !== shown;
    shown = key;
    if (fresh) stuck = true;
    const go = requestAnimationFrame(() => {
      if (writing) { if (stuck) el.scrollTop = el.scrollHeight; }
      // a saved change: the newest one, from its start
      else if (fresh) el.scrollTop = Math.max(0, (el.querySelector<HTMLElement>('.part:last-of-type')?.offsetTop ?? 0) - 6);
    });
    return () => cancelAnimationFrame(go);
  });
  let touchY = 0;
  const letGo = () => { if (draft) stuck = false; };
  function onScroll() {
    if (code && code.scrollTop + code.clientHeight >= code.scrollHeight - 4) stuck = true;
  }

  function openIt() {
    if (!path) return;
    void chat.editors.open(path);
    live.seen(path);
    if (app.narrow) app.inspectorDrawer = false;
  }
  const inTree = $derived(!!path && !path.startsWith('/') && !path.startsWith('~') && !path.startsWith('..'));
  const jump = $derived(!live.follow && live.target && live.target !== path ? live.target : null);
</script>

<section class="lp" aria-label={t('live.label')}>
  {#if path}
    <div class="hd">
      <i class="sq {failed ? 'bad' : draft ? 'run' : mark}" aria-hidden="true"></i>
      <span class="nm mono" data-priv="files" title={showcase.tip('files', path)}>{baseName(path)}</span>
      {#if word}<span class="st" class:bad={!!failed} class:ok={!failed && !draft} role="status">{word}</span>{/if}
      {#if counts && (counts.added || counts.removed)}
        <span class="num">{#if counts.added}<b class="p">+{counts.added}</b>{/if}{#if counts.removed} <b class="m">−{counts.removed}</b>{/if}</span>
      {/if}
      <span class="grow"></span>
      {#if inTree && !failed && !(draft && !change && !disk)}
        <button class="icon-btn sm" type="button" title={t('live.open')} aria-label={t('live.open')} onclick={openIt}><Icon name="file" size={15} /></button>
      {/if}
      <button class="flw" type="button" aria-pressed={live.follow} title={t('live.followHint')} onclick={() => live.setFollow(!live.follow)}>{t('live.follow')}</button>
    </div>
    <div class="win">
      <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
      <div class="code hl" class:dead={!!failed} tabindex="0" role="group" aria-label={baseName(path)} bind:this={code} onscroll={onScroll}
        onwheel={(e) => { if (e.deltaY < 0) letGo(); }} ontouchstart={(e) => (touchY = e.touches[0].clientY)} ontouchmove={(e) => { if (e.touches[0].clientY > touchY + 4) letGo(); }}
        onkeydown={(e) => { if (['ArrowUp', 'PageUp', 'Home'].includes(e.key)) letGo(); }}>
        {#if failed?.text && !failed.declined}<p class="say bad">{failed.text}</p>{/if}
        {#if note}<p class="say">{note}</p>{/if}
        {#if draft?.draft.append}<p class="say">{t('live.append')}</p>{/if}
        {#if disk && disk.k === 'mod' && parts.length}<p class="say">{t('live.fromDisk')}</p>{/if}
        {#if earlier}<p class="say">{t('live.earlier', { n: earlier })}</p>{/if}
        {#each parts as p, i (i)}
          <div class="part">
            {#if p.head}<p class="say">{p.head}</p>{/if}
            {#if p.above}<p class="say">{t('live.above', { n: p.above })}</p>{/if}
            {#each p.lines as l, j (p.above != null ? l.n ?? j : j)}
              {#if l.k === 'gap'}<div class="ln gap">⋯</div>
              {:else}<div class="ln {l.k}"><span class="g" class:sign={l.n == null} aria-hidden="true">{l.n ?? (l.k === 'add' ? '+' : l.k === 'del' ? '−' : '')}</span><span class="x">{@html paint(l.x)}{#if draft?.phase === 'composing' && i === parts.length - 1 && j === p.lines.length - 1}<i class="caret"></i>{/if}</span></div>{/if}
            {/each}
            {#if p.below}<p class="say">{t('live.below', { n: p.below })}</p>{/if}
          </div>
        {/each}
      </div>
      {#if jump}
        <button class="jump" type="button" onclick={() => live.setFollow(true)}><Icon name="down" size={14} />{t('live.jump')} <span class="mono" data-priv="files">{baseName(jump)}</span></button>
      {/if}
    </div>
    {#if live.history.length}
      <div class="hist" role="group" aria-label={t('live.history')}>
        {#each live.history.slice(0, 40) as h (h)}
          <button type="button" class:on={h === path} aria-pressed={h === path} title={showcase.tip('files', h)} onclick={() => live.pick(h)}>
            <i class="sq {live.kind(h)}" class:old={!live.marks[h]} aria-hidden="true"></i><FileIcon name={baseName(h)} size={13} /><span class="mono" data-priv="files">{baseName(h)}</span>
          </button>
        {/each}
      </div>
    {/if}
  {/if}
</section>

<style>
  .lp { flex: 1; min-height: 0; min-width: 0; display: flex; flex-direction: column; }
  .hd { flex: none; display: flex; align-items: center; gap: 8px; min-width: 0; height: 36px; padding: 0 12px 4px 14px; font-size: 13px; }
  .nm { min-width: 0; font-size: 12.5px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .st { flex: none; color: var(--ink-3); font-size: 12.5px; white-space: nowrap; }
  .st.ok { color: var(--ok); }
  .st.bad { color: var(--bad); }
  .num { flex: none; font: 12px var(--font-mono); font-variant-numeric: tabular-nums; white-space: nowrap; }
  .num b { font-weight: 500; }
  .num .p { color: var(--ok); }
  .num .m { color: var(--bad); }
  .grow { flex: 1; }
  .sm { width: 26px; height: 26px; flex: none; }
  .flw { flex: none; height: 26px; padding: 0 9px; border-radius: 6px; font-size: 12.5px; color: var(--ink-3); white-space: nowrap; }
  .flw[aria-pressed='true'] { color: var(--accent); background: var(--accent-wash); }
  .sq { flex: none; width: 8px; height: 8px; border-radius: 2px; background: var(--rule); }
  .sq.run { background: var(--heat); }
  .sq.new { background: var(--ok); }
  .sq.mod { background: transparent; box-shadow: inset 0 0 0 1.5px var(--accent); }
  .sq.bad { background: var(--bad); }
  .sq.old { opacity: 0.45; }
  .win { flex: 1; min-height: 0; position: relative; display: flex; margin: 0 12px; border: 1px solid var(--rule); border-radius: 10px; overflow: hidden; background: var(--code-bg); }
  .win:last-child { margin-bottom: 12px; }
  .code { flex: 1; min-width: 0; overflow: auto; overscroll-behavior: contain; padding: 6px 0 20px; font: 12px/1.6 var(--font-mono); position: relative; }
  .code:focus-visible { outline: 2px solid color-mix(in srgb, var(--accent) 60%, transparent); outline-offset: -2px; border-radius: 10px; }
  .code.dead .part { opacity: 0.5; }
  .part { min-width: max-content; }
  .part + .part { margin-top: 6px; }
  .ln { display: flex; min-width: max-content; white-space: pre; border-left: 3px solid transparent; tab-size: 4; }
  .g { width: 42px; flex: none; padding-right: 12px; text-align: right; color: var(--ink-3); opacity: 0.75; user-select: none; }
  .g.sign { width: 24px; padding: 0; text-align: center; opacity: 1; }
  .x { padding-right: 20px; min-height: 1.6em; }
  .add { background: var(--add-bg); border-left-color: var(--ok); }
  .del { background: var(--del-bg); border-left-color: var(--bad); }
  .ctx { color: var(--ink-2); }
  .gap { padding-left: 50px; color: var(--ink-3); }
  .say { margin: 0; padding: 4px 14px; color: var(--ink-3); font: 11.5px/1.5 var(--font-ui, inherit); position: sticky; left: 0; white-space: normal; }
  .say.bad { color: var(--bad); }
  .caret { display: inline-block; width: 7px; height: 1.1em; margin-left: 1px; vertical-align: -2px; border-radius: 1px; background: var(--heat); }
  .jump {
    position: absolute; left: 50%; bottom: 10px; transform: translateX(-50%); z-index: 2;
    display: inline-flex; align-items: center; gap: 6px; height: 32px; max-width: calc(100% - 20px); padding: 0 14px 0 10px;
    border: 1px solid var(--rule); border-radius: 999px; background: var(--raised); color: var(--ink); font-size: 13px; white-space: nowrap; overflow: hidden;
  }
  .jump .mono { font-size: 12.5px; min-width: 0; overflow: hidden; text-overflow: ellipsis; }
  .hist { flex: none; display: flex; gap: 6px; overflow-x: auto; padding: 8px 12px 12px; }
  .hist button { flex: none; display: inline-flex; align-items: center; gap: 6px; height: 28px; max-width: 220px; padding: 0 10px; border: 1px solid var(--rule); border-radius: 8px; font-size: 12px; color: var(--ink-2); }
  .hist button.on { background: var(--raised); color: var(--ink); }
  .hist .mono { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  @media (hover: hover) { .hist button:hover { color: var(--ink); } }
</style>
