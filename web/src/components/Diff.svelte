<script lang="ts">
  // pi's edit tool returns a ready-made diff: "+ 52 text", "- 48 text", "  47 text", "     ..." for gaps.
  let { diff }: { diff: string } = $props();
  const lines = $derived(diff.split('\n').map((raw) => ({ raw, kind: raw[0] === '+' ? 'add' : raw[0] === '-' ? 'del' : 'ctx' })));
</script>

<div class="diff" role="group" aria-label="diff">
  {#each lines as l, i (i)}
    <div class="ln {l.kind}"><span class="sign" aria-hidden="true">{l.raw[0] === '+' || l.raw[0] === '-' ? l.raw[0] : ' '}</span><span class="txt">{l.raw.slice(1)}</span></div>
  {/each}
</div>

<style>
  .diff {
    font: 12.5px/1.55 var(--font-mono);
    background: var(--code-bg);
    border-radius: 6px;
    overflow-x: auto;
    padding: 6px 0;
    max-height: 420px;
    overflow-y: auto;
  }
  .ln {
    display: flex;
    min-width: max-content;
    white-space: pre;
    padding-right: 14px;
  }
  .sign {
    width: 22px;
    flex: none;
    text-align: center;
    color: var(--ink-3);
    user-select: none;
  }
  .add {
    background: var(--add-bg);
  }
  .add .sign {
    color: var(--ok);
  }
  .del {
    background: var(--del-bg);
  }
  .del .sign {
    color: var(--bad);
  }
  .ctx .txt {
    color: var(--ink-2);
  }
</style>
