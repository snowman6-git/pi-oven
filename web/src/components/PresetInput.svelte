<script lang="ts">
  import { t } from '../lib/i18n.svelte';
  import { dismiss } from '../lib/actions';
  import Icon from './Icon.svelte';

  // A number field with a list of common values under a button: each value with a one-line example of what it suits.
  // Typing any number still works; picking one fills the field.
  type Preset = { value: number; label: string; desc: string };
  let { value = $bindable(''), presets, placeholder = '', label }: { value?: string; presets: Preset[]; placeholder?: string; label: string } = $props();

  let open = $state(false);
  const fmt = (n: number) => n.toLocaleString();
  function pick(p: Preset) {
    value = String(p.value);
    open = false;
  }
</script>

<div class="pi" use:dismiss={() => (open = false)}>
  <div class="row">
    <input class="field" inputmode="numeric" bind:value {placeholder} aria-label={label} />
    <button class="icon-btn" type="button" aria-label={t('models.presets', { what: label })} aria-haspopup="listbox" aria-expanded={open} onclick={() => (open = !open)}>
      <Icon name="down" size={16} />
    </button>
  </div>
  {#if open}
    <ul class="list" role="listbox" aria-label={label}>
      {#each presets as p (p.value)}
        <li>
          <button type="button" role="option" aria-selected={value === String(p.value)} class:on={value === String(p.value)} onclick={() => pick(p)}>
            <span class="v">{p.label}<small>{fmt(p.value)}</small></span>
            <span class="d">{p.desc}</span>
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .pi {
    position: relative;
  }
  .row {
    display: flex;
    gap: 4px;
  }
  .row .field {
    flex: 1;
    min-width: 0;
  }
  .list {
    position: absolute;
    left: 0;
    right: 0;
    top: calc(100% + 4px);
    z-index: 30;
    list-style: none;
    margin: 0;
    padding: 4px;
    max-height: 280px;
    overflow: auto;
    background: var(--raised);
    border: 1px solid var(--rule);
    border-radius: 10px;
    box-shadow: var(--shadow);
  }
  .list button {
    width: 100%;
    display: grid;
    gap: 1px;
    text-align: left;
    padding: 7px 10px;
    border-radius: 7px;
  }
  .list button.on {
    background: var(--accent-wash);
  }
  @media (hover: hover) {
    .list button:hover {
      background: var(--paper-sunk);
    }
  }
  .v {
    color: var(--ink);
    font-size: 13.5px;
    font-weight: 600;
  }
  .v small {
    margin-left: 8px;
    font-weight: 400;
    color: var(--ink-3);
    font-family: var(--font-mono);
    font-size: 11.5px;
  }
  /* the same grey as the reset times on the plan-limit cards */
  .d {
    color: var(--ink-3);
    font-size: 12px;
    font-weight: 400;
  }
</style>
