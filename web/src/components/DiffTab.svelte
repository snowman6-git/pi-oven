<script lang="ts">
  import { untrack } from 'svelte';
  import type { Chat } from '../lib/chat.svelte';
  import { contentAsDiff, runningEdits, sessionChanges, shortPath } from '../lib/changes';
  import Diff from './Diff.svelte';
  import FileIcon from './FileIcon.svelte';

  let { chat }: { chat: Chat } = $props();

  // What pi changed, read from its own edit and write calls as they arrive. Nothing is read from disk:
  // each edit's diff is laid over the file as it happens, which also shows the order things were done in.
  const files = $derived(sessionChanges(chat.items));
  const busy = $derived(runningEdits(chat.items));
  let openPath = $state<string | null>(null);
  let follow = $state(true);

  const latest = $derived(files.reduce<typeof files[number] | null>((a, f) => (!a || f.lastSeq > a.lastSeq ? f : a), null));
  const total = $derived(files.reduce((n, f) => n + f.touches.length, 0));

  // A new edit lands: open that file, so the change is on screen as it happens.
  let seen = 0;
  $effect(() => {
    const n = total;
    const path = latest?.path;
    untrack(() => {
      if (n > seen && seen > 0 && follow && path) openPath = path;
      seen = n;
    });
  });
  let box = $state<HTMLElement>();
  $effect(() => {
    total;
    if (follow && openPath) queueMicrotask(() => box?.querySelector('[data-latest="true"]')?.scrollIntoView({ block: 'nearest' }));
  });

  const pathOf = (p: string) => shortPath(p, chat.runtime.cwd);
  const dir = (p: string) => (p.includes('/') ? p.slice(0, p.lastIndexOf('/') + 1) : '');
  const name = (p: string) => p.slice(p.lastIndexOf('/') + 1);
</script>

<div class="tab" bind:this={box}>
  <div class="bar">
    <p>{files.length ? `${files.length}개 파일, 수정 ${total}번` : 'pi가 고친 파일이 여기에 나타납니다.'}</p>
    <label class="follow" title="새 수정이 생기면 그 파일을 자동으로 펼칩니다"><input type="checkbox" bind:checked={follow} /> 따라가기</label>
  </div>

  {#each busy as p (p)}
    <div class="working" role="status"><span class="spin" aria-hidden="true"></span><span class="mono">{pathOf(p)}</span> 수정 중…</div>
  {/each}

  <ul>
    {#each files as f (f.path)}
      {@const open = openPath === f.path}
      <li class:open>
        <button class="row" type="button" aria-expanded={open} onclick={() => (openPath = open ? null : f.path)}>
          <span class="p mono" title={pathOf(f.path)}><span class="fic"><FileIcon name={name(f.path)} size={14} /></span><span class="nm" data-priv="files">{name(f.path)}</span>{#if dir(pathOf(f.path))} <span class="d" data-priv="files">{dir(pathOf(f.path))}</span>{/if}</span>
          <span class="n"><b class="add">+{f.added}</b> <b class="del">−{f.removed}</b></span>
        </button>
        {#if open}
          <div class="body">
            {#each f.touches as t, i (i)}
              <div class="touch" data-latest={f === latest && i === f.touches.length - 1}>
                {#if f.touches.length > 1}<small>{i + 1}번째 {t.kind === 'write' ? '쓰기' : '수정'}</small>{/if}
                {#if t.kind === 'edit'}
                  {#if t.diff}<Diff diff={t.diff} />{:else}<small>diff가 기록되어 있지 않습니다.</small>{/if}
                {:else}
                  <small>파일 전체를 새로 썼습니다.</small>
                  <Diff diff={contentAsDiff(t.content ?? '')} />
                {/if}
              </div>
            {/each}
          </div>
        {/if}
      </li>
    {/each}
  </ul>
  {#if files.length}<p class="foot"><code class="mono">bash</code>로 바뀐 파일은 여기 없습니다. Git 탭에서 보세요.</p>{/if}
</div>

<style>
  .tab { display: grid; gap: 10px; min-width: 0; }
  .bar { display: flex; justify-content: space-between; align-items: center; gap: 8px; }
  p { margin: 0; color: var(--ink-3); font-size: 13px; }
  .follow { display: flex; align-items: center; gap: 5px; font-size: 12.5px; color: var(--ink-2); white-space: nowrap; }
  .follow input { accent-color: var(--accent); }
  ul { list-style: none; margin: 0; padding: 0; display: grid; gap: 4px; }
  li { border: 1px solid var(--rule); border-radius: 8px; background: var(--paper); min-width: 0; }
  li.open { border-color: var(--ink-3); }
  .row { width: 100%; display: flex; justify-content: space-between; align-items: baseline; gap: 8px; padding: 7px 10px; text-align: left; }
  .p { min-width: 0; font-size: 12.5px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .d { color: var(--ink-3); margin-left: 7px; }
  .fic { display: inline-flex; vertical-align: -3px; margin-right: 7px; }
  .n { flex: none; font-size: 12px; }
  .add { color: var(--ok); font-weight: 600; }
  .del { color: var(--bad); font-weight: 600; }
  .body { padding: 0 8px 8px; display: grid; gap: 8px; min-width: 0; }
  .touch { display: grid; gap: 4px; min-width: 0; }
  .touch small { color: var(--ink-3); font-size: 11.5px; }
  .body :global(.diff) { max-height: 40dvh; font-size: 12px; }
  .working { display: flex; align-items: center; gap: 8px; padding: 7px 10px; border-radius: 8px; background: var(--accent-wash); color: var(--accent); font-size: 12.5px; min-width: 0; }
  .working .mono { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .spin { flex: none; width: 10px; height: 10px; border-radius: 50%; border: 2px solid currentColor; border-right-color: transparent; animation: dt-spin 0.8s linear infinite; }
  @keyframes dt-spin { to { rotate: 360deg; } }
  @media (prefers-reduced-motion: reduce) { .spin { animation-duration: 3s; } }
  .foot { font-size: 12px; }
</style>
