<script lang="ts">
  import { copyText } from '../lib/clipboard';
  import { renderMarkdown } from '../lib/markdown';
  import { settings } from '../lib/settings.svelte';
  import { Reveal } from '../lib/stream-fx';
  import { S } from '../lib/strings';

  let { source, streaming = false }: { source: string; streaming?: boolean } = $props();

  // Re-parsing on every token is wasteful; coalesce to one parse per frame while streaming.
  const reveal = new Reveal();
  let html = $state('');
  let frame = 0;

  function render(src: string) {
    const out = renderMarkdown(src, !streaming);
    if (settings.fx === 'none') return out;
    if (streaming) reveal.started = true;
    // Text that was never streamed (history) stays still; text that was keeps animating to the end.
    return reveal.started ? reveal.apply(out) : out;
  }

  $effect(() => {
    const src = source;
    settings.fx;
    if (!streaming) {
      html = render(src);
      return;
    }
    cancelAnimationFrame(frame);
    frame = requestAnimationFrame(() => (html = render(src)));
    return () => cancelAnimationFrame(frame);
  });

  function onclick(e: MouseEvent) {
    const btn = (e.target as HTMLElement).closest<HTMLButtonElement>('button[data-copy]');
    if (!btn) return;
    const code = btn.closest('.code')?.querySelector('code')?.textContent ?? '';
    void copyText(code).then((ok) => {
      btn.textContent = ok ? S.copied : '복사 실패';
      setTimeout(() => (btn.textContent = S.copy), 1400);
    });
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="md" class:streaming data-fx={settings.fx} {onclick}>{@html html}</div>

<style>
  .md {
    overflow-wrap: anywhere;
    line-height: 1.72;
  }
  .md :global(> :first-child) {
    margin-top: 0;
  }
  .md :global(> :last-child) {
    margin-bottom: 0;
  }
  .md :global(p),
  .md :global(ul),
  .md :global(ol),
  .md :global(blockquote),
  .md :global(table),
  .md :global(.code) {
    margin: 0 0 0.9em;
  }
  .md :global(h1),
  .md :global(h2),
  .md :global(h3),
  .md :global(h4) {
    margin: 1.4em 0 0.5em;
    line-height: 1.3;
    letter-spacing: -0.01em;
  }
  .md :global(h1) {
    font-size: 1.35em;
  }
  .md :global(h2) {
    font-size: 1.18em;
  }
  .md :global(h3),
  .md :global(h4) {
    font-size: 1.04em;
  }
  .md :global(ul),
  .md :global(ol) {
    padding-left: 1.4em;
  }
  .md :global(li + li) {
    margin-top: 0.25em;
  }
  .md :global(a) {
    color: var(--accent);
    text-underline-offset: 3px;
  }
  .md :global(blockquote) {
    padding-left: 1em;
    border-left: 2px solid var(--rule);
    color: var(--ink-2);
  }
  .md :global(:not(pre) > code) {
    padding: 0.12em 0.4em;
    border-radius: 4px;
    background: var(--code-bg);
  }
  .md :global(hr) {
    border: 0;
    border-top: 1px solid var(--rule);
    margin: 1.4em 0;
  }
  .md :global(table) {
    border-collapse: collapse;
    display: block;
    overflow-x: auto;
    font-size: 0.94em;
  }
  .md :global(th),
  .md :global(td) {
    padding: 6px 12px;
    border-bottom: 1px solid var(--rule);
    text-align: left;
  }
  .md :global(th) {
    font-weight: 650;
  }
  .md :global(.code) {
    border-radius: 6px;
    background: var(--code-bg);
    overflow: hidden;
  }
  .md :global(.code-bar) {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 4px 6px 4px 12px;
    font-size: 12px;
    color: var(--ink-3);
    border-bottom: 1px solid var(--rule);
  }
  .md :global(.code-bar button) {
    padding: 2px 8px;
    border-radius: 5px;
    color: var(--ink-2);
    font-size: 12px;
  }
  @media (hover: hover) {
    .md :global(.code-bar button:hover) {
      background: var(--paper-sunk);
      color: var(--ink);
    }
  }

  .md :global(pre) {
    margin: 0;
    padding: 12px 14px;
    overflow-x: auto;
    line-height: 1.55;
    tab-size: 2;
  }
  .md :global(pre code) {
    font-size: 0.84em;
  }
  /* Syntax colours follow the palette instead of a stock highlight theme. */
  .md :global(.hljs-comment),
  .md :global(.hljs-quote) {
    color: var(--ink-3);
    font-style: italic;
  }
  .md :global(.hljs-keyword),
  .md :global(.hljs-selector-tag),
  .md :global(.hljs-built_in) {
    color: var(--accent);
  }
  .md :global(.hljs-string),
  .md :global(.hljs-attr),
  .md :global(.hljs-addition) {
    color: var(--ok);
  }
  .md :global(.hljs-number),
  .md :global(.hljs-literal),
  .md :global(.hljs-symbol) {
    color: var(--heat);
  }
  .md :global(.hljs-title),
  .md :global(.hljs-function),
  .md :global(.hljs-section) {
    font-weight: 650;
  }
  .md :global(.hljs-deletion) {
    color: var(--bad);
  }
  .md :global(.hljs-meta),
  .md :global(.hljs-type) {
    color: var(--ink-2);
  }
</style>
