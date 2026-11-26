<script lang="ts">
  // What the server did while nobody was working (compacted the session, or acted on a plan nearly used): one quiet
  // line across the conversation, in the Baton Touch line's shape but without its orange.
  import { restText, type RestNote } from '../lib/rest';

  let { note }: { note: RestNote } = $props();
  const text = $derived(restText(note));
</script>

<div class="rest" role="note" aria-label={text}>
  <i class="rl" aria-hidden="true"></i>
  <span class="txt">{text}</span>
  <i class="rl" aria-hidden="true"></i>
</div>

<style>
  .rest {
    display: flex;
    align-items: center;
    gap: 12px;
    margin: 4px 0 0;
    min-width: 0;
  }
  .rl {
    flex: 1 1 16px;
    min-width: 12px;
    height: 1px;
    background: var(--rule);
  }
  .txt {
    flex: 0 1 auto;
    min-width: 0;
    max-width: 80%;
    font-size: 12px;
    color: var(--ink-3);
    text-align: center;
    text-wrap: balance;
  }
</style>
