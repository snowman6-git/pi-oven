<script lang="ts">
  // One small bar per agent of a subagent run, in its colour: done (green), working (amber, with the slow glow),
  // waiting (the rule), failed (red). The same marks in the chip, the tray and the sheet. `dot` makes it the point
  // a flight leaves from or lands on (BgFlight looks for `.dot`).
  import type { Step } from '../addons/subagents';

  let { steps, max = 6, dot = false, tall = 14 }: { steps: Step[]; max?: number; dot?: boolean; tall?: number } = $props();
  // with more agents than fit, the first `max - 1` show and the last bar stands for the rest (its colour: the busiest)
  const shown = $derived.by(() => {
    if (steps.length <= max) return steps.map((s) => s.status);
    const head = steps.slice(0, max - 1).map((s) => s.status);
    const rest = steps.slice(max - 1);
    const tail = rest.some((s) => s.status === 'running') ? 'running' : rest.some((s) => s.status === 'failed') ? 'failed' : rest.every((s) => s.status === 'done') ? 'done' : 'pending';
    return [...head, tail];
  });
</script>

<span class="segs" class:dot aria-hidden="true" style="--tall:{tall}px">
  {#each shown as st, i (i)}<i class={st}></i>{/each}
</span>

<style>
  .segs {
    flex: none;
    display: inline-flex;
    gap: 3px;
  }
  i {
    width: 5px;
    height: var(--tall);
    border-radius: 2px;
    background: var(--rule);
  }
  i.done {
    background: var(--ok);
  }
  i.failed {
    background: var(--bad);
  }
  i.running {
    background: var(--heat);
    position: relative;
  }
  i.running::after {
    content: '';
    position: absolute;
    inset: 0;
    border-radius: inherit;
    box-shadow: 0 0 0 3px var(--heat-ring);
    opacity: 0;
    pointer-events: none;
    will-change: opacity;
    animation: sa-ember 1.2s ease-in-out infinite;
  }
  /* composited: a fixed ring that only fades */
  @keyframes sa-ember {
    50% {
      opacity: 1;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    i.running::after {
      animation: none;
    }
  }
</style>
