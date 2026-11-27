<script lang="ts">
  // Subagent runs as a bar over the message box, like the task list (Settings → Theme → Subagents → Tray; chips are
  // the default). Folded, one line follows the newest run going: who, in order, what the one at work is doing, and
  // for how long. Opened, every run is listed in full.
  import { showIn } from '../lib/showin';
  import { t as tr } from '../lib/i18n.svelte';
  import { runClock } from '../lib/util';
  import { showcase } from '../lib/showcase.svelte';
  import type { Chat } from '../lib/chat.svelte';
  import { runCount, runName, type Run } from '../addons/subagents';
  import Icon from './Icon.svelte';
  import SaSegs from './SaSegs.svelte';
  import SaRun from './SaRun.svelte';

  let { chat }: { chat: Chat } = $props();
  const sa = $derived(chat.sa);
  const runs = $derived(sa.chips);

  let open = $state(read());
  function read() { try { return localStorage.getItem('pi-oven.sa-open') === 'on'; } catch { return false; } }
  function toggle() {
    open = !open;
    try { localStorage.setItem('pi-oven.sa-open', open ? 'on' : 'off'); } catch { /* private mode */ }
  }

  /** The run the folded line follows: the newest one going, else the newest. */
  const lead = $derived([...runs].reverse().find((r) => r.state === 'running') ?? runs[runs.length - 1]);
  const going = $derived(runs.filter((r) => r.state === 'running').length);
  const busy = $derived(lead ? (lead.steps.find((s) => s.status === 'running') ?? lead.steps[lead.steps.length - 1]) : undefined);

  let now = $state(Date.now());
  $effect(() => {
    if (!going) return;
    now = Date.now();
    const t = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(t);
  });
  const showCall = (r: Run) => showIn(document.querySelector(`[data-tool="${CSS.escape(r.toolId)}"]`));
</script>

{#if lead}
  <div class="satray" class:open data-sa-tray role="region" aria-label={tr('sa.title')}>
    <button class="bar" type="button" aria-expanded={open} aria-controls="sa-list" onclick={toggle}>
      {#if open}
        <b class="title">{tr('sa.title')}</b>
        <span class="sum">{tr('sa.trayCount', { n: runs.length })}{going ? ' · ' + tr('sa.groupRunning', { n: going }) : ''}</span>
      {:else}
        <span class="dot"><SaSegs steps={lead.steps} /></span>
        {#if lead.shape === 'chain'}
          <span class="flow">
            {#each lead.steps as s, i (i)}
              {#if i > 0}<span class="arr" aria-hidden="true">→</span>{/if}
              <span class="who {s.status}">{s.agent}</span>
            {/each}
            <span class="cnt">{runCount(lead)}</span>
          </span>
        {:else}
          <span class="who running">{runName(lead)}</span>
        {/if}
        <span class="what" data-priv="paths">{lead.state === 'running' && busy?.tool ? `${busy.tool}${busy.toolArgs ? ' ' + showcase.text(busy.toolArgs) : ''}` : ''}</span>
        {#if runs.length > 1}<span class="more">+{runs.length - 1}</span>{/if}
        <span class="clock" class:go={lead.state === 'running'}>{lead.state === 'running' ? runClock(Math.max(0, now - lead.startedAt)) : tr(lead.state === 'done' ? 'sa.done' : 'sa.failed')}</span>
      {/if}
      <span class="chev" class:down={open}><Icon name="up" size={14} /></span>
    </button>
    {#if open}
      <div id="sa-list" class="list">
        {#each [...runs].reverse() as r (r.toolId)}
          <div class="row"><SaRun run={r} {now} onshow={() => showCall(r)} /></div>
        {/each}
      </div>
    {/if}
  </div>
{/if}

<style>
  .satray {
    max-width: calc(var(--measure) + 74px);
    margin: 0 auto 8px;
    border: 1px solid var(--rule);
    border-radius: 10px;
    background: var(--raised);
    overflow: hidden;
  }
  .satray.open {
    border-radius: 12px;
  }
  .bar {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 10px;
    height: 38px;
    padding: 0 6px 0 14px;
    font-size: 13px;
    text-align: left;
    color: var(--ink-2);
  }
  .satray.open .bar {
    border-bottom: 1px solid var(--rule);
  }
  .dot {
    flex: none;
    display: inline-flex;
  }
  .title {
    font-weight: 600;
    color: var(--ink);
  }
  .sum {
    color: var(--ink-3);
  }
  .flow {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    flex: none;
    max-width: 55%;
    overflow: hidden;
    white-space: nowrap;
  }
  .arr {
    color: var(--ink-3);
  }
  .cnt {
    display: none;
    color: var(--ink-3);
  }
  /* narrow: only the one at work and how far along ("worker 2/3"), not the whole line of names */
  @media (max-width: 560px) {
    .flow .arr,
    .flow .who:not(.running) {
      display: none;
    }
    .cnt {
      display: inline;
    }
  }
  .who {
    white-space: nowrap;
    color: var(--ink-3);
  }
  .who.done {
    color: var(--ok);
  }
  .who.failed {
    color: var(--bad);
  }
  .who.running {
    color: var(--ink);
    font-weight: 560;
  }
  .what {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--font-mono);
    font-size: 12px;
    color: var(--ink-3);
  }
  .more {
    flex: none;
    padding: 1px 7px;
    border-radius: 999px;
    background: var(--rule);
    color: var(--ink-2);
    font-size: 11px;
  }
  .clock {
    flex: none;
    margin-left: auto;
    color: var(--ink-3);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
  }
  .clock.go {
    color: var(--heat);
    font-family: var(--font-mono);
  }
  .chev {
    flex: none;
    width: 28px;
    display: grid;
    place-items: center;
    color: var(--ink-3);
    transition: transform 0.18s;
  }
  .chev.down {
    transform: rotate(180deg);
  }
  .open .chev {
    margin-left: auto;
  }
  .list {
    max-height: min(320px, 45vh);
    overflow: auto;
  }
  .row {
    padding: 12px 14px;
  }
  .row + .row {
    border-top: 1px solid var(--rule);
  }
</style>
