<script lang="ts">
  let { percent }: { percent: number | null } = $props();
  const R = 7;
  const C = 2 * Math.PI * R;
  const shown = $derived(percent == null ? 0 : Math.max(0, Math.min(100, percent)));
</script>

<svg width="20" height="20" viewBox="0 0 20 20" aria-hidden="true" class:warm={shown > 75}>
  <circle cx="10" cy="10" r={R} class="track" />
  <circle cx="10" cy="10" r={R} class="arc" stroke-dasharray={C} stroke-dashoffset={C * (1 - shown / 100)} transform="rotate(-90 10 10)" />
</svg>

<style>
  circle {
    fill: none;
    stroke-width: 2.4;
  }
  .track {
    stroke: var(--rule);
  }
  .arc {
    stroke: var(--accent);
    stroke-linecap: round;
    transition: stroke-dashoffset 0.4s;
  }
  .warm .arc {
    stroke: var(--heat);
  }
</style>
