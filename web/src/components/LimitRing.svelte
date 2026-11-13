<script lang="ts">
  import type { WindowNow } from '../lib/limits';

  // A ring cut in two by a straight upright groove: the left half is the long window (a week), the right half the
  // short one (5 hours). Each fills from the bottom up. The groove is cut with clip paths so its sides stay
  // parallel; ends are square. Each half shows what is left, so it drains as the plan is used. A "!" sits in the
  // groove once a window is used up.
  let { short, long, size = 20 }: { short?: WindowNow; long?: WindowNow; size?: number } = $props();
  const id = $props.id();

  /** How much of each half the groove hides at either end, in % of the half circle. */
  const END = 5.5;
  const dash = (w?: WindowNow) => (w && w.left > 0 ? `${(END + (w.left * (100 - 2 * END)) / 100).toFixed(1)} 100` : '0 100');
  const LEFT = 'M10 17 A7 7 0 0 1 10 3';
  const RIGHT = 'M10 17 A7 7 0 0 0 10 3';
  const full = $derived(!!(short?.reached || long?.reached));
</script>

<svg width={size} height={size} viewBox="0 0 20 20" aria-hidden="true">
  <defs>
    <clipPath id="{id}l"><rect x="0" y="0" width="8.8" height="20" /></clipPath>
    <clipPath id="{id}r"><rect x="11.2" y="0" width="8.8" height="20" /></clipPath>
  </defs>
  <g clip-path="url(#{id}l)">
    <path d={LEFT} class="track" class:full={long?.reached} />
    {#if long && !long.reached && long.left > 0}<path d={LEFT} pathLength="100" class="arc {long.level}" stroke-dasharray={dash(long)} />{/if}
  </g>
  <g clip-path="url(#{id}r)">
    <path d={RIGHT} class="track" class:full={short?.reached} />
    {#if short && !short.reached && short.left > 0}<path d={RIGHT} pathLength="100" class="arc {short.level}" stroke-dasharray={dash(short)} />{/if}
  </g>
  {#if full}<text x="10" y="13.4" text-anchor="middle">!</text>{/if}
</svg>

<style>
  path {
    fill: none;
    stroke-width: 2.4;
    stroke-linecap: butt;
  }
  .track {
    stroke: var(--rule);
  }
  .track.full {
    stroke: var(--bad);
  }
  .arc {
    stroke: var(--accent);
    transition: stroke-dasharray 0.4s;
  }
  .arc.warm {
    stroke: var(--heat);
  }
  text {
    fill: var(--bad);
    font: 800 9.5px system-ui, sans-serif;
  }
  @media (prefers-reduced-motion: reduce) {
    .arc {
      transition: none;
    }
  }
</style>
