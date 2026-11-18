<script lang="ts">
  // Baton Touch in the conversation: a rule across it, where another account or model takes over from here on. The
  // name it was starts in the middle, then shrinks up into a small "from" line while the new name rises into its
  // place (once, when it has just arrived; drawn from history it is already there).
  import { t } from '../lib/i18n.svelte';
  import { clock } from '../lib/util';
  import { showcase } from '../lib/showcase.svelte';
  import { batonKind, type BatonNote } from '../lib/baton';
  import type { ModelInfo } from '../lib/protocol';

  let { note, fresh = false, models = [] }: { note: BatonNote; fresh?: boolean; models?: ModelInfo[] } = $props();

  const kind = $derived(batonKind(note));
  const isModel = $derived(kind === 'model' || kind === 'back');
  // a model by its name when this pi knows it
  const named = (id?: string | null) => (id ? (models.find((m) => m.id === id)?.name ?? id) : '');
  const who = $derived(isModel ? named(note.model) : (note.email || note.provider || ''));
  const was = $derived(isModel ? named(note.was) : (note.was ?? ''));
  const line = $derived(t(`accounts.line.${kind}`, { at: note.at ? clock(note.at) : '' }));
  const priv = $derived(isModel ? undefined : 'user');
</script>

<div class="baton" class:fresh role="note" aria-label={`Baton Touch · ${was ? t('accounts.lineFrom', { who: was }) + ' → ' : ''}${who} · ${line}`}>
  <i class="rl l" aria-hidden="true"></i>
  <div class="mid">
    <span class="lbl" aria-hidden="true">BATON TOUCH</span>
    {#if was}<span class="was" data-priv={priv}>{showcase.text(t('accounts.lineFrom', { who: was }))}</span>{/if}
    <span class="who" data-priv={priv} title={note.plan ?? undefined}>{showcase.text(who)}</span>
    <span class="sub">{line}</span>
  </div>
  <i class="rl r" aria-hidden="true"></i>
</div>

<style>
  .baton {
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
    background: color-mix(in srgb, var(--accent) 55%, transparent);
  }
  .mid {
    flex: 0 1 auto;
    min-width: 0;
    max-width: 80%;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 1px;
    text-align: center;
  }
  .lbl {
    font: 600 10.5px var(--font-mono);
    letter-spacing: 0.12em;
    color: var(--accent);
  }
  .was {
    font-size: 11.5px;
    color: var(--ink-3);
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .who {
    font-size: 13.5px;
    font-weight: 600;
    color: var(--ink);
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sub {
    font-size: 11.5px;
    color: var(--ink-3);
    text-wrap: balance;
  }
  /* just arrived: the rules draw out from the middle, the old name hands its place down, the new one rises */
  .fresh .rl.l {
    transform-origin: right;
    animation: draw 0.55s ease-out 0.1s both;
  }
  .fresh .rl.r {
    transform-origin: left;
    animation: draw 0.55s ease-out 0.1s both;
  }
  .fresh .mid {
    animation: pop 0.5s cubic-bezier(0.3, 1.5, 0.5, 1) both;
  }
  .fresh .was {
    animation: hand-up 0.55s cubic-bezier(0.3, 1.5, 0.5, 1) 0.35s both;
  }
  .fresh .who {
    animation: rise-in 0.5s cubic-bezier(0.3, 1.5, 0.5, 1) 0.7s both;
  }
  .fresh .sub {
    animation: fade-up 0.3s ease-out 1s both;
  }
  @keyframes draw {
    from { transform: scaleX(0); }
  }
  @keyframes pop {
    0% { transform: scale(0.92); opacity: 0; }
    60% { transform: scale(1.03); opacity: 1; }
    100% { transform: none; }
  }
  @keyframes hand-up {
    from { transform: translateY(17px) scale(1.18); color: var(--ink); }
  }
  @keyframes rise-in {
    from { transform: translateY(8px); opacity: 0; }
  }
  @keyframes fade-up {
    from { transform: translateY(4px); opacity: 0; }
  }
  @media (prefers-reduced-motion: reduce) {
    .fresh .rl, .fresh .mid, .fresh .was, .fresh .who, .fresh .sub { animation: none; }
  }
</style>
