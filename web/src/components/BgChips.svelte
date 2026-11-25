<script lang="ts">
  import { t as tr } from '../lib/i18n.svelte';
  import type { Chat } from '../lib/chat.svelte';
  import type { BgTask } from '../lib/protocol';
  import { runClock } from '../lib/util';
  import { settings } from '../lib/settings.svelte';
  import { dismiss } from '../lib/actions';
  import { trackPopover } from '../lib/popover.svelte';
  import { grow, shrink } from '../lib/sheet';
  import { runCount, runName, type Run } from '../addons/subagents';
  import SaSegs from './SaSegs.svelte';
  import SaRun from './SaRun.svelte';

  let { chat }: { chat: Chat } = $props();

  // One row above the message box. Subagent runs come first, as squarer chips with a bar per agent (Settings → Theme
  // can put them in a tray instead); a thin rule; then background tasks as round chips. Kept apart, so the row reads
  // as two groups however they start and end.
  const sa = $derived(chat.sa);
  const runs = $derived(settings.subagentView === 'chips' ? sa.chips : []);
  // Chips sit side by side in one row. How many background chips fit follows what the subagents leave; the rest go to the circle.
  let rowW = $state(0);
  let saW = $state(0);
  const CHIP_MIN = 150; // narrowest a chip may get before its name is no use
  const SA_CHIP = 190; // a run's chip with a short name: bars, name, n/m and its clock
  /** With more than two runs at once they fold into one chip ("에이전트 5"); its sheet lists them all. Two fold as
      well when they would not fit at a useful width beside the background tasks (a phone). */
  const folded = $derived(runs.length > 2 || (runs.length === 2 && rowW > 0 && rowW < 2 * SA_CHIP + (chat.bgChips.length ? CHIP_MIN + 6 : 0) + 40));
  const fit = $derived(rowW ? Math.max(1, Math.floor((rowW - 36 - (runs.length ? saW + 17 : 0)) / (CHIP_MIN + 6))) : 4);

  // A running task shows how long it has been running (the server's start time, so a reload or another device
  // carries on from the same number). Clocks that differ by more than the task's age can't be told apart here.
  let now = $state(Date.now());
  const anyRunning = $derived(chat.bgChips.some((t) => t.status === 'running') || runs.some((r) => r.state === 'running') || !!sa.open);
  $effect(() => {
    if (!anyRunning) return;
    now = Date.now();
    const t = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(t);
  });
  const elapsed = (t: BgTask) => (typeof t.startTime === 'number' ? Math.max(0, now - t.startTime) : 0);

  // the sheet: opened by its chip, closed by the same chip, a click elsewhere, Escape or the phone's back button
  const toggleSa = (id: string) => (sa.open = sa.open === id ? null : id);
  $effect(() => {
    if (sa.open) return trackPopover(() => (sa.open = null));
  });
  // the run it was opened for is gone (handed over): nothing to show
  $effect(() => {
    if (sa.open && sa.open !== '*' && !sa.runs.some((r) => r.toolId === sa.open)) sa.open = null;
    if (sa.open === '*' && !runs.length) sa.open = null;
  });
  const sheetRuns = $derived(sa.open === '*' ? sa.chips : sa.runs.filter((r) => r.toolId === sa.open));
  const allSteps = $derived(runs.flatMap((r) => r.steps));
  const goingRuns = $derived(runs.filter((r) => r.state === 'running').length);
  const showCall = (r: Run) => {
    sa.open = null;
    document.querySelector(`[data-tool="${CSS.escape(r.toolId)}"]`)?.scrollIntoView({ block: 'center', behavior: 'smooth' });
  };
  const runBad = (r: Run) => r.state === 'failed';
</script>

{#if chat.bgChips.length || runs.length}
  {@const chips = chat.bgChips}
  {@const rest = chips.slice(fit).filter((t) => !chat.bgLanding.includes(t.id))}
  <div class="sawrap" use:dismiss={() => { if (sa.open) sa.open = null; }}>
  <div class="bgrow" role="group" aria-label={tr('bg.title')} bind:clientWidth={rowW}>
    {#if runs.length}
      <span class="sagroup" class:shared={chips.length > 0} role="group" aria-label={tr('sa.title')} bind:clientWidth={saW}>
        {#if folded}
          <span class="sachip" data-sa-group class:run={goingRuns > 0} class:on={sa.open === '*'}>
            <button class="main" type="button" aria-haspopup="dialog" aria-expanded={sa.open === '*'} onclick={() => toggleSa('*')}>
              <span class="dot"><SaSegs steps={allSteps} /></span>
              <span class="nm">{tr('sa.group', { n: runs.length })}</span>
              {#if goingRuns && goingRuns < runs.length}<span class="ct">{tr('sa.groupRunning', { n: goingRuns })}</span>{/if}
            </button>
          </span>
        {:else}
          {#each runs as r (r.toolId)}
            {@const going = r.state === 'running'}
            <span class="sachip" data-sa-chip={r.toolId} class:land={sa.landing.includes(r.toolId)} class:fading={sa.fading.includes(r.toolId)}
              class:run={going} class:bad={runBad(r)} class:on={sa.open === r.toolId}>
              <button class="main" type="button" aria-haspopup="dialog" aria-expanded={sa.open === r.toolId} onclick={() => toggleSa(r.toolId)}>
                <span class="dot"><SaSegs steps={r.steps} /></span>
                <span class="nm">{runName(r)}</span>
                {#if runCount(r)}<span class="ct">{runCount(r)}</span>{/if}
                {#if going && now - r.startedAt >= 1000}
                  <span class="st tm">{runClock(Math.max(0, now - r.startedAt))}</span>
                {:else if !going}
                  <span class="st">{tr(r.state === 'done' ? 'sa.done' : 'sa.failed')}</span>
                {/if}
              </button>
              {#if !going}
                <button class="x" type="button" aria-label={tr('chips.dismiss', { name: runName(r) })} onclick={() => sa.dismiss(r.toolId)}>×</button>
              {/if}
            </span>
          {/each}
        {/if}
      </span>
      {#if chips.length}<span class="sep" aria-hidden="true"></span>{/if}
    {/if}
    {#each chips.slice(0, fit) as t (t.id)}
      <span class="bgchip" data-bg-chip={t.id} class:land={chat.bgLanding.includes(t.id)} class:fading={chat.bgFading.includes(t.id)} class:run={t.status === 'running'} class:bad={t.status === 'failed' || t.status === 'killed'}>
        <button class="main" type="button" onclick={() => { chat.bgFocus = t.id; chat.bgOpen = true; chat.dismissBg(t.id); }} aria-haspopup="dialog"
          title={t.command.split('\n')[0]}>
          <span class="dot" role="img" aria-label={tr(`bg.status.${t.status}`)}></span>
          <span class="nm">{t.name || t.description || t.command.split('\n')[0]}</span>
          {#if t.status === 'running' && elapsed(t) >= 1000}
            <span class="st tm">{runClock(elapsed(t))}</span>
          {:else}
            <span class="st">{tr(`bg.status.${t.status}`)}</span>
          {/if}
        </button>
        {#if t.status !== 'running'}
          <button class="x" type="button" aria-label={tr('chips.dismiss', { name: t.name || tr('chips.task') })} onclick={() => chat.dismissBg(t.id)}>×</button>
        {/if}
      </span>
    {/each}
    <!-- What does not fit in the one row piles up in one small circle. It is there (hidden) before the first one is stacked, so a task has somewhere to land. -->
    {#if chips.length > fit}
      <button class="stack" data-bg-stack class:land={rest.length === 0} class:run={rest.some((t) => t.status === 'running')} class:bad={rest.some((t) => t.status === 'failed' || t.status === 'killed')}
        type="button" aria-label={tr('chips.more', { n: rest.length })} title={tr('chips.more', { n: rest.length })} aria-haspopup="dialog" onclick={() => (chat.bgOpen = true)}>+{rest.length}</button>
    {/if}
  </div>
  {#if sa.open && sheetRuns.length}
    <div class="sheet sasheet" in:grow out:shrink role="dialog" aria-label={tr('sa.title')}>
      {#each sheetRuns as r (r.toolId)}
        <div class="sarow"><SaRun run={r} {now} onshow={() => showCall(r)} /></div>
      {/each}
    </div>
  {/if}
  </div>
{/if}

<style>
  .bgrow {
    max-width: calc(var(--measure) + 74px);
    margin: 0 auto 8px;
  }
  .bgrow {
    display: flex;
    flex-wrap: nowrap;
    align-items: center;
    gap: 6px;
    width: 100%;
  }
  .bgchip {
    display: inline-flex;
    align-items: center;
    flex: 0 1 auto;
    min-width: 0;
    max-width: 220px;
    height: 30px;
    border-radius: 999px;
    border: 1px solid var(--rule);
    background: var(--raised);
    color: var(--ink-2);
    font-size: 13px;
    font-weight: 560;
  }
  .bgchip.land {
    visibility: hidden;
  }
  .bgchip.fading {
    opacity: 0;
    scale: 0.9;
    transition: opacity 0.3s, scale 0.3s;
  }
  .bgchip .main {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    height: 100%;
    padding: 0 12px 0 10px;
    border-radius: 999px;
  }
  .bgchip .nm {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .bgchip .st {
    flex: none;
    color: var(--ink-3);
    font-weight: 500;
  }
  /* amber: only while the task is actually running */
  .bgchip .st.tm {
    color: var(--heat);
    font-family: var(--font-mono); /* the UI font's digits differ in width: a ticking clock would shake the chip */
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    font-weight: 500;
    min-width: 4ch;
    text-align: right;
  }
  .bgchip .x {
    flex: none;
    width: 24px;
    height: 24px;
    margin-right: 3px;
    border-radius: 50%;
    color: var(--ink-3);
    font-size: 16px;
    line-height: 1;
  }
  .bgchip .x:hover {
    background: var(--rule);
  }
  /* One circle for everything that does not fit, same height as a chip so it is a true circle. */
  .stack {
    flex: none;
    width: 30px;
    height: 30px;
    border-radius: 50%;
    border: 1px solid var(--rule);
    background: var(--raised);
    color: var(--ink-2);
    font-size: 12px;
    font-weight: 650;
    display: grid;
    place-items: center;
    font-variant-numeric: tabular-nums;
  }
  .stack.land {
    visibility: hidden;
  }
  .stack.run {
    color: var(--ink);
    border-color: var(--heat-edge);
  }
  .stack.bad {
    border-color: color-mix(in srgb, var(--bad) 60%, var(--rule));
  }
  .bgchip .dot {
    flex: none;
    width: 8px;
    height: 8px;
    border-radius: 2px;
    background: var(--ok);
  }
  .bgchip.run {
    color: var(--ink);
    border-color: var(--heat-edge);
  }
  .bgchip.run .dot {
    background: var(--heat);
    position: relative;
  }
  .bgchip.bad .dot {
    background: var(--bad);
  }
  .sawrap {
    display: contents;
  }
  .sagroup {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    max-width: 100%;
    min-width: 0;
  }
  /* sharing the row with background tasks: both give way only when the row runs out (too narrow for two runs at
     all, and they fold into one chip) */
  .sagroup.shared {
    flex: 0 1 auto;
  }
  .sep {
    flex: none;
    width: 1px;
    height: 18px;
    margin: 0 4px;
    background: var(--rule);
  }
  /* a subagent run: squarer than a background task's chip, with a bar per agent instead of one dot */
  .sachip {
    display: inline-flex;
    align-items: center;
    flex: 0 1 auto;
    min-width: 0;
    max-width: 240px;
    height: 30px;
    border-radius: 8px;
    border: 1px solid var(--rule);
    background: var(--raised);
    color: var(--ink-2);
    font-size: 13px;
    font-weight: 560;
  }
  .sachip.run {
    color: var(--ink);
  }
  .sachip.bad {
    border-color: color-mix(in srgb, var(--bad) 60%, var(--rule));
  }
  .sachip.on {
    border-color: var(--accent);
    background: var(--accent-wash);
  }
  .sachip.land {
    visibility: hidden;
  }
  .sachip.fading {
    opacity: 0;
    scale: 0.9;
    transition: opacity 0.3s, scale 0.3s;
  }
  .sachip .main {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    height: 100%;
    padding: 0 12px 0 8px;
    border-radius: 8px;
  }
  .sachip .dot {
    flex: none;
    display: inline-flex;
  }
  .sachip .nm {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sachip .ct,
  .sachip .st {
    flex: none;
    color: var(--ink-3);
    font-weight: 500;
  }
  .sachip .st.tm {
    color: var(--heat);
    font-family: var(--font-mono);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    min-width: 4ch;
    text-align: right;
  }
  .sachip .x {
    flex: none;
    width: 24px;
    height: 24px;
    margin-right: 3px;
    border-radius: 50%;
    color: var(--ink-3);
    font-size: 16px;
    line-height: 1;
  }
  @media (hover: hover) {
    .sachip .x:hover {
      background: var(--rule);
    }
  }
  .sasheet {
    gap: 0;
    padding: 0;
  }
  .sarow {
    padding: 14px 16px;
  }
  .sarow + .sarow {
    border-top: 1px solid var(--rule);
  }
  /* the first one takes the free space above, so they sit at the bottom and the top ones show as the sheet grows */
  .sarow:first-child {
    margin-top: auto;
  }
</style>
