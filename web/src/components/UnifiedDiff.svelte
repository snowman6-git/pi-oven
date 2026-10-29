<script lang="ts">
  // `git diff` output: file headers and hunk markers in a quiet colour, additions and removals tinted.
  let { text }: { text: string } = $props();
  const kind = (l: string) =>
    l.startsWith('@@') ? 'hunk'
      : l.startsWith('+++') || l.startsWith('---') || l.startsWith('diff ') || l.startsWith('index ') || l.startsWith('new file') || l.startsWith('deleted file') || l.startsWith('similarity') || l.startsWith('rename ') ? 'head'
      : l[0] === '+' ? 'add' : l[0] === '-' ? 'del' : 'ctx';
  const lines = $derived(text.replace(/\n$/, '').split('\n').map((raw) => ({ raw, k: kind(raw) })));
</script>

<div class="diff" role="group" aria-label="diff">
  {#each lines as l, i (i)}
    <div class="ln {l.k}">{l.raw || ' '}</div>
  {/each}
</div>

<style>
  .diff {
    font: 12.5px/1.55 var(--font-mono);
    background: var(--code-bg);
    border-radius: 8px;
    overflow: auto;
    padding: 6px 0;
  }
  .ln {
    min-width: max-content;
    white-space: pre;
    padding: 0 14px;
    color: var(--ink-2);
  }
  .add { background: var(--add-bg); color: var(--ink); }
  .del { background: var(--del-bg); color: var(--ink); }
  .head { color: var(--ink-3); }
  .hunk { color: var(--accent); margin-top: 4px; }
</style>
