<script lang="ts">
  // One subagent run, opened up: who is in it (one after another with arrows, or side by side), what the one at work
  // was asked and what it is doing now, and the way back to the call in the conversation.
  import { t } from '../lib/i18n.svelte';
  import { showcase } from '../lib/showcase.svelte';
  import { compactNumber, duration, runClock } from '../lib/util';
  import type { Run, Step } from '../addons/subagents';

  let { run, now, onshow }: { run: Run; now: number; onshow?: () => void } = $props();

  const busy = $derived(run.steps.find((s) => s.status === 'running') ?? run.steps.find((s) => s.status === 'pending') ?? run.steps[run.steps.length - 1]);
  const going = $derived(run.state === 'running');
  const took = $derived(going ? Math.max(0, now - run.startedAt) : (run.endedAt ?? now) - run.startedAt);
  /** "턴 19 · 도구 30회 · 컨텍스트 131k": how much an agent has done, so a long quiet stretch reads as work, not a hang. */
  const meta = (s: Step) =>
    [s.turns ? t('sa.turns', { n: s.turns }) : '', s.tools ? t('sa.tools', { n: s.tools }) : '', s.context ? t('sa.ctx', { n: compactNumber(s.context) }) : '']
      .filter(Boolean)
      .join(' · ');
</script>

<div class="sarun">
  <div class="flow" class:side={run.shape === 'parallel'}>
    <!-- side by side, each agent has its own line below: the head keeps only the clock -->
    {#if run.shape !== 'parallel'}
      {#each run.steps as s, i (i)}
        {#if i > 0 && run.shape === 'chain'}<span class="arr" aria-hidden="true">→</span>{/if}
        <span class="step {s.status}"><i aria-hidden="true"></i>{s.agent}<span class="sr">{t(`sa.step.${s.status}`)}</span></span>
      {/each}
    {:else}
      <span class="ptitle">{t('sa.side', { n: run.steps.length })}</span>
    {/if}
    <span class="clock" class:go={going}>{going ? runClock(took) : t(run.state === 'done' ? 'sa.done' : 'sa.failed') + ' · ' + duration(took)}</span>
  </div>
  {#if run.shape === 'parallel'}
    <!-- side by side: a line for each agent, since they all work at once -->
    <ul class="kids">
      {#each run.steps as s, i (i)}
        <li class={s.status}>
          <span class="who"><i aria-hidden="true"></i>{s.agent}</span>
          <span class="col">
            {#if s.task}<span class="ktask" data-priv="chat">{showcase.text(s.task)}</span>{/if}
            {#if s.status === 'running' && s.tool}<span class="kdo mono" data-priv="paths"><span class="lbl">{t(s.recent ? 'sa.recent' : 'sa.now')}</span>{s.tool}{s.toolArgs ? ' ' + showcase.text(s.toolArgs) : ''}</span>{/if}
            {#if meta(s)}<span class="kmeta">{meta(s)}</span>{/if}
          </span>
        </li>
      {/each}
    </ul>
  {:else}
    {#if busy?.task}<p class="task" data-priv="chat">{showcase.text(busy.task)}</p>{/if}
    {#if going && busy?.tool}
      <p class="now"><span class="lbl">{t(busy.recent ? 'sa.recent' : 'sa.now')}</span><span class="mono" data-priv="paths">{busy.tool}{busy.toolArgs ? ' ' + showcase.text(busy.toolArgs) : ''}</span></p>
    {/if}
    {#if busy && meta(busy)}<p class="meta">{meta(busy)}</p>{/if}
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
  .ptitle {
    color: var(--ink-3);
    font-size: 12px;
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
  .meta,
  .kmeta {
    margin: 0;
    font-size: 12px;
    color: var(--ink-3);
    font-variant-numeric: tabular-nums;
  }
  .kids {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .kids li {
    display: flex;
    gap: 10px;
    min-width: 0;
    font-size: 13px;
  }
  .who {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    width: 96px;
    height: 20px;
    color: var(--ink-2);
  }
  .who i {
    width: 8px;
    height: 8px;
    border-radius: 2px;
    background: var(--rule);
  }
  .running .who {
    color: var(--ink);
    font-weight: 560;
  }
  .running .who i {
    background: var(--heat);
  }
  .done .who i {
    background: var(--ok);
  }
  .failed .who i {
    background: var(--bad);
  }
  .col {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 3px;
    line-height: 20px;
  }
  .ktask {
    color: var(--ink);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .done .ktask,
  .pending .ktask {
    color: var(--ink-2);
  }
  .kdo {
    display: flex;
    gap: 8px;
    font-size: 12px;
    color: var(--ink-2);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .kdo .lbl {
    color: var(--ink-3);
    font-family: var(--font-ui);
  }
  .mono {
    font-family: var(--font-mono);
  }
  .acts {
    display: flex;
    justify-content: flex-end;
  }
</style>
