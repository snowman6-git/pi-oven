<script lang="ts">
  import type { Snippet } from 'svelte';
  import Icon from './Icon.svelte';
  import type { PATHS } from '../lib/icons';
  import { S } from '../lib/strings';

  // One card under the session list, for anything that can be updated (pi and its packages, pi-oven itself).
  // The text opens the matching section; the button is that card's own action. Several cards stack: pi-oven's own first, then pi's.
  let {
    title,
    detail = '',
    detailTitle,
    mono = false,
    state = 'idle',
    onopen,
    action,
    onclose,
    extra,
  }: {
    title: string;
    detail?: string;
    detailTitle?: string;
    mono?: boolean;
    /** busy: a spinner; done: a check; bad: the title in red */
    state?: 'idle' | 'busy' | 'done' | 'bad';
    onopen?: () => void;
    action?: { label: string; run: () => void; primary?: boolean; icon?: keyof typeof PATHS };
    onclose?: () => void;
    extra?: Snippet;
  } = $props();
</script>

<div class="upd" role="status">
  {#if state === 'busy'}<span class="spin" aria-hidden="true"></span>
  {:else}<Icon name={state === 'done' ? 'check' : 'update'} size={18} />{/if}
  {#if onopen}
    <button class="ut open" type="button" onclick={onopen}>
      <b class:bad={state === 'bad'}>{title}</b>{#if detail}<span class:mono title={detailTitle}>{detail}</span>{/if}
    </button>
  {:else}
    <div class="ut"><b class:bad={state === 'bad'}>{title}</b>{#if detail}<span class:mono title={detailTitle}>{detail}</span>{/if}{@render extra?.()}</div>
  {/if}
  {#if action || onclose}
    <div class="ua row2">
      {#if action}
        <button class="btn" class:primary={action.primary !== false} class:quiet={action.primary === false} type="button" onclick={action.run}>
          {#if action.icon}<Icon name={action.icon} size={14} />{/if}{action.label}
        </button>
      {/if}
      {#if onclose}<button class="icon-btn sm" type="button" aria-label={S.close} title={S.close} onclick={onclose}><Icon name="x" size={15} /></button>{/if}
    </div>
  {/if}
  {#if onopen && extra}<div class="more">{@render extra()}</div>{/if}
</div>

<style>
  .upd {
    display: grid;
    grid-template-columns: 18px minmax(0, 1fr) auto;
    align-items: center;
    gap: 10px;
    padding: 10px 10px 10px 12px;
    border: 1px solid var(--rule);
    border-radius: 10px;
    background: var(--raised);
    color: var(--accent);
  }
  .ut {
    min-width: 0;
    display: grid;
    gap: 1px;
    line-height: 1.35;
    color: var(--ink);
    text-align: left;
  }
  .ut.open {
    margin: -4px;
    padding: 4px;
    border-radius: 6px;
  }
  @media (hover: hover) {
    .ut.open:hover {
      background: var(--paper-sunk);
    }
  }
  .ut b {
    font-size: 13.5px;
    font-weight: 650;
  }
  .ut b.bad {
    color: var(--bad);
  }
  .ut span {
    font-size: 12px;
    color: var(--ink-3);
    word-break: keep-all;
    overflow-wrap: anywhere;
  }
  .ut span.mono {
    font-family: var(--font-mono);
    font-size: 11.5px;
  }
  .more {
    grid-column: 2 / -1;
    min-width: 0;
  }
  .ua {
    display: flex;
    align-items: center;
    gap: 2px;
  }
  .ua.row2 {
    grid-column: 2 / -1;
    margin-top: 2px;
  }
  .ua.row2 .icon-btn {
    margin-left: auto;
  }
  .ua .btn {
    height: 28px;
    padding: 0 10px;
    font-size: 12.5px;
    gap: 5px;
  }
  .spin {
    width: 16px;
    height: 16px;
    border-radius: 50%;
    border: 2px solid color-mix(in srgb, var(--heat) 25%, transparent);
    border-top-color: var(--heat);
    animation: spin 0.9s linear infinite;
  }
  @keyframes spin {
    to {
      rotate: 360deg;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .spin {
      animation-duration: 3s;
    }
  }
</style>
