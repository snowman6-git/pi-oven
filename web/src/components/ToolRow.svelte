<script lang="ts">
  import type { ToolBlock } from '../lib/chat.svelte';
  import { app } from '../lib/app.svelte';
  import { showcase } from '../lib/showcase.svelte';
  import { contextMenu, type MenuItem } from '../lib/contextmenu.svelte';
  import { S } from '../lib/strings';
  import { duration, runClock } from '../lib/util';
  import Diff from './Diff.svelte';

  let { tool }: { tool: ToolBlock } = $props();
  let open = $state(false);
  let expanded = $state(false);

  const summary = $derived.by(() => {
    const a = tool.args ?? {};
    switch (tool.name) {
      case 'bash': return String(a.command ?? '').split('\n')[0];
      case 'read': return a.path ? `${a.path}${a.offset ? `:${a.offset}` : ''}` : '';
      case 'write': case 'edit': return String(a.path ?? '');
      case 'todo': {
        const what = a.subject ?? (Array.isArray(a.tasks) ? `${a.tasks.length}개` : '');
        return [a.action, a.id != null ? `#${a.id}` : '', what, a.status ? `→ ${a.status}` : ''].filter(Boolean).join(' ');
      }
      default: {
        const first = Object.values(a).find((v) => typeof v === 'string') as string | undefined;
        return first ? first.split('\n')[0].slice(0, 140) : '';
      }
    }
  });

  const toolItems = (): MenuItem[] => [
    { label: tool.name === 'bash' ? '명령 복사' : '경로·요약 복사', icon: tool.name === 'bash' ? 'terminal' : 'link', disabled: !summary, run: () => app.copy(tool.name === 'bash' ? String(tool.args.command ?? '') : summary) },
    { label: '인자(JSON) 복사', icon: 'code', run: () => app.copy(JSON.stringify(tool.args, null, 2)) },
    { label: '출력 복사', icon: 'copy', disabled: !tool.output, run: () => app.copy(tool.output) },
    ...(diff ? [{ label: '변경 내용(diff) 복사', icon: 'copy', run: () => app.copy(diff) } as MenuItem] : []),
    'sep',
    { label: open ? '접기' : '펼치기', icon: open ? 'up' : 'down', run: () => (open = !open) },
  ];

  // A call that takes a while shows how long it has been running (after 3s, so quick ones stay quiet). The time it
  // ends with is the one pi measured.
  let now = $state(Date.now());
  $effect(() => {
    if (tool.status !== 'running' || !tool.startedAt) return;
    now = Date.now();
    const t = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(t);
  });
  const running = $derived(tool.status === 'running' && tool.startedAt ? now - tool.startedAt : 0);

  const MAX_LINES = 28;
  const outLines = $derived(tool.output ? tool.output.split('\n') : []);
  const clipped = $derived(!expanded && outLines.length > MAX_LINES);
  const shown = $derived(clipped ? outLines.slice(0, MAX_LINES).join('\n') : tool.output);
  const tail = $derived(outLines.slice(-3).join('\n'));
  const diff = $derived(tool.name === 'edit' && typeof tool.details?.diff === 'string' ? (tool.details.diff as string) : '');
  const written = $derived(tool.name === 'write' && typeof tool.args?.content === 'string' ? (tool.args.content as string) : '');
  const label = $derived(
    tool.status === 'running' ? '실행 중' : tool.status === 'error' ? '실패' : tool.status === 'ok' ? '완료' : '대기',
  );
</script>

<div class="tool-row" data-status={tool.status} data-tool={tool.id}>
  <button class="head" type="button" aria-expanded={open} onclick={() => (open = !open)} use:contextMenu={toolItems}>
    <span class="dot" role="img" aria-label={label}></span>
    <span class="name">{tool.name}</span>
    <span class="sum mono" data-priv="files">{showcase.text(summary)}</span>
    {#if running >= 3000}<span class="dur live" aria-hidden="true">{runClock(running)}</span>{:else}<span class="dur">{duration(tool.durationMs)}</span>{/if}
  </button>

  {#if tool.status === 'running' && !open && tool.name === 'bash' && tail}
    <pre class="peek mono">{tail}</pre>
  {/if}

  {#if open}
    <div class="detail">
      {#if tool.name === 'bash'}
        <pre class="out mono"><span class="prompt">$ </span>{showcase.text(String(tool.args.command ?? ''))}</pre>
      {:else if !diff && !written}
        <pre class="out mono args">{showcase.text(JSON.stringify(tool.args, null, 2))}</pre>
      {/if}

      {#if diff}
        <Diff {diff} />
      {:else if written}
        <pre class="out mono">{showcase.text(written.length > 4000 && !expanded ? written.slice(0, 4000) + '\n…' : written)}</pre>
      {/if}

      {#if tool.output && !diff && tool.name !== 'write'}
        <pre class="out mono" class:err={tool.status === 'error'}>{showcase.text(shown)}</pre>
        {#if outLines.length > MAX_LINES}
          <button class="more" type="button" onclick={() => (expanded = !expanded)}>
            {expanded ? S.showLess : `${S.showMore} (${outLines.length - MAX_LINES}줄)`}
          </button>
        {/if}
      {:else if tool.status === 'error' && tool.output}
        <pre class="out mono err">{showcase.text(tool.output)}</pre>
      {/if}

      {#each tool.images ?? [] as img}
        <img class="shot" alt="" src={`data:${img.mimeType};base64,${img.data}`} />
      {/each}
    </div>
  {/if}
</div>

<style>
  .tool-row:global(+ .tool-row) {
    border-top: 1px solid var(--rule);
  }
  .head {
    width: 100%;
    display: grid;
    grid-template-columns: 14px auto minmax(0, 1fr) auto;
    align-items: center;
    gap: 10px;
    padding: 6px 4px;
    text-align: left;
    border-radius: 4px;
  }
  @media (hover: hover) {
    .head:hover {
      background: color-mix(in srgb, var(--paper-sunk) 70%, transparent);
    }
  }

  .dot {
    width: 8px;
    height: 8px;
    border-radius: 2px;
    justify-self: center;
    border: 1.5px solid var(--ink-3);
  }
  [data-status='ok'] .dot {
    background: var(--ok);
    border-color: var(--ok);
  }
  [data-status='error'] .dot {
    background: var(--bad);
    border-color: var(--bad);
  }
  [data-status='running'] .dot {
    background: var(--heat);
    border-color: var(--heat);
    animation: ember 1.1s ease-in-out infinite;
  }
  @keyframes ember {
    50% {
      box-shadow: 0 0 0 4px color-mix(in srgb, var(--heat) 28%, transparent);
    }
  }
  .name {
    font-weight: 650;
    font-size: 13.5px;
  }
  .sum {
    color: var(--ink-2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    font-size: 12.5px;
  }
  .dur {
    color: var(--ink-3);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  /* amber: only while the work is actually running */
  .dur.live {
    color: var(--heat);
  }
  .peek {
    margin: 0 0 6px 28px;
    color: var(--ink-3);
    font-size: 12px;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    max-height: 4.8em;
    overflow: hidden;
  }
  .detail {
    margin: 2px 0 10px 28px;
    display: grid;
    gap: 8px;
  }
  .out {
    margin: 0;
    padding: 10px 12px;
    background: var(--code-bg);
    border-radius: 6px;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    max-height: 420px;
    overflow: auto;
    font-size: 12.5px;
    line-height: 1.55;
  }
  .out.args {
    color: var(--ink-2);
  }
  .out.err {
    color: var(--bad);
  }
  .prompt {
    color: var(--accent);
    user-select: none;
  }
  .more {
    justify-self: start;
    font-size: 12.5px;
    color: var(--accent);
    padding: 2px 4px;
  }
  .shot {
    max-width: min(100%, 420px);
    border-radius: 6px;
    border: 1px solid var(--rule);
  }
</style>
