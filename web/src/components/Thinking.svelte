<script lang="ts">
  import { settings } from '../lib/settings.svelte';
  import { escapeHtml, Reveal } from '../lib/stream-fx';
  import { S } from '../lib/strings';

  let { text, redacted = false, live = false }: { text: string; redacted?: boolean; live?: boolean } = $props();

  // Same word-by-word effect as the answer text; history (never streamed) stays plain.
  const reveal = new Reveal();
  let html = $state('');
  let frame = 0;
  $effect(() => {
    const t = text;
    const fx = settings.fx;
    if (live) reveal.started = true;
    const render = () => (html = fx !== 'none' && reveal.started ? reveal.applyText(t) : escapeHtml(t));
    if (!live) {
      render();
      return;
    }
    cancelAnimationFrame(frame);
    frame = requestAnimationFrame(render);
    return () => cancelAnimationFrame(frame);
  });

</script>

<details class="think" open={live && settings.thinkingOpen}>
  <summary>{S.thinking}{#if redacted} <em>(숨김)</em>{:else if text} <em>{text.length.toLocaleString()}자</em>{/if}</summary>
  {#if text}<p class:live data-fx={settings.fx}>{@html html}</p>{/if}
</details>

<style>
  .think {
    margin: 0 0 10px;
    color: var(--ink-2);
    font-size: 14px;
  }
  summary {
    cursor: pointer;
    list-style: none;
    display: inline-flex;
    gap: 6px;
    align-items: baseline;
    color: var(--ink-3);
    font-weight: 560;
  }
  summary::-webkit-details-marker {
    display: none;
  }
  summary::before {
    content: '';
    width: 6px;
    height: 6px;
    border-right: 1.5px solid currentColor;
    border-bottom: 1.5px solid currentColor;
    transform: rotate(-45deg);
    transition: transform 0.12s;
  }
  .think[open] summary::before {
    transform: rotate(45deg);
  }
  em {
    font-style: normal;
    font-weight: 400;
  }
  p {
    margin: 6px 0 0 3px;
    padding-left: 14px;
    border-left: 2px solid var(--rule);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    max-height: 220px;
    overflow: auto;
    /* Words rise from 0.75em below their line; this keeps that travel inside the box so the
       scrollable height does not swell and collapse with every word. */
    padding-bottom: 1em;
    overflow-anchor: none;
    font-style: italic;
  }
  /* While it is being written the block simply grows and the conversation scrolls with it, so
     new words stay where the eye is. Only a finished, re-opened block is height-limited. */
  p.live {
    max-height: none;
    overflow: visible;
  }
</style>
