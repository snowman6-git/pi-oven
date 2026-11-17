<script lang="ts">
  // One subagent run, opened up: who is in it (one after another with arrows, or side by side), what the one at work
  // was asked and what it is doing now, and the way back to the call in the conversation.
  import { t } from '../lib/i18n.svelte';
  import { showcase } from '../lib/showcase.svelte';
  import { duration, runClock } from '../lib/util';
  import type { Run } from '../addons/subagents';

  let { run, now, onshow }: { run: Run; now: number; onshow?: () => void } = $props();

  const busy = $derived(run.steps.find((s) => s.status === 'running') ?? run.steps.find((s) => s.status === 'pending') ?? run.steps[run.steps.length - 1]);
  const going = $derived(run.state === 'running');
  const took = $derived(going ? Math.max(0, now - run.startedAt) : (run.endedAt ?? now) - run.startedAt);
</script>

<div class="sarun">
  <div class="flow" class:side={run.shape === 'parallel'}>
    {#each run.steps as s, i (i)}
      {#if i > 0 && run.shape === 'chain'}<span class="arr" aria-hidden="true">→</span>{/if}
      <span class="step {s.status}"><i aria-hidden="true"></i>{s.agent}<span class="sr">{t(`sa.step.${s.status}`)}</span></span>
    {/each}
    <span class="clock" class:go={going}>{going ? runClock(took) : t(run.state === 'done' ? 'sa.done' : 'sa.failed') + ' · ' + duration(took)}</span>
  </div>
  {#if busy?.task}<p class="task" data-priv="chat">{showcase.text(busy.task)}</p>{/if}
  {#if going && busy?.tool}
    <p class="now"><span class="lbl">{t('sa.now')}</span><span class="mono" data-priv="paths">{busy.tool}{busy.toolArgs ? ' ' + showcase.text(busy.toolArgs) : ''}</span></p>
  {/if}
  {#if onshow}
    <div class="acts"><button class="btn quiet" type="button" onclick={onshow}>{t('sa.showCall')}</button></div>
  {/if}
</div>

<style>
  .sarun {
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-width: 0;
  }
  .flow {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    font-size: 13px;
  }
  .arr {
    color: var(--ink-3);
  }
  .step {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 3px 9px;
    border: 1px solid var(--rule);
    border-radius: 8px;
    color: var(--ink-2);
  }
  .step i {
    width: 8px;
    height: 8px;
    border-radius: 2px;
    background: var(--rule);
  }
  .step.done {
    color: var(--ok);
  }
  .step.done i {
    background: var(--ok);
  }
  .step.failed {
    color: var(--bad);
  }
  .step.failed i {
    background: var(--bad);
  }
  .step.running {
    color: var(--ink);
    border-color: var(--heat);
    background: var(--heat-wash);
  }
  .step.running i {
    background: var(--heat);
  }
  .step.pending {
    border-style: dashed;
    color: var(--ink-3);
  }
  .sr {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
  .clock {
    margin-left: auto;
    color: var(--ink-3);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
  }
  .clock.go {
    color: var(--heat);
    font-family: var(--font-mono);
  }
  .task {
    margin: 0;
    font-size: 14px;
    line-height: 1.5;
    color: var(--ink);
    overflow-wrap: anywhere;
  }
  .now {
    margin: 0;
    display: flex;
    gap: 8px;
    min-width: 0;
    font-size: 12px;
    color: var(--ink-2);
  }
  .now .lbl {
    flex: none;
    color: var(--ink-3);
  }
  .now .mono {
    font-family: var(--font-mono);
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .acts {
    display: flex;
    justify-content: flex-end;
  }
</style>
