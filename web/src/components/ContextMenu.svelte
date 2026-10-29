<script lang="ts">
  import { tick } from 'svelte';
  import { menu } from '../lib/contextmenu.svelte';
  import Icon from './Icon.svelte';

  let box = $state<HTMLElement>();
  let left = $state(0);
  let top = $state(0);
  let active = $state(-1);

  const actionable = (i: number) => {
    const it = menu.items[i];
    return it !== undefined && it !== 'sep' && !it.disabled;
  };

  // Place at the pointer, then pull back inside the window (flip above if there is no room below).
  $effect(() => {
    if (!menu.open) return;
    active = -1;
    const { x, y, sheet } = menu;
    tick().then(() => {
      if (!box) return;
      box.focus({ preventScroll: true });
      if (sheet) return;
      const pad = 8, w = box.offsetWidth, h = box.offsetHeight;
      left = Math.max(pad, Math.min(x, innerWidth - w - pad));
      top = y + h + pad > innerHeight ? Math.max(pad, y - h) : y;
    });
  });

  // Anything that moves the page under the menu, or leaves it, dismisses it.
  $effect(() => {
    if (!menu.open) return;
    const hide = () => menu.hide();
    const away = (e: PointerEvent) => { if (!box?.contains(e.target as Node)) menu.hide(); };
    addEventListener('pointerdown', away, true);
    addEventListener('keydown', onkeydown, true);
    addEventListener('wheel', hide, { capture: true, passive: true });
    addEventListener('scroll', hide, true);
    addEventListener('resize', hide);
    addEventListener('blur', hide);
    return () => {
      removeEventListener('pointerdown', away, true);
      removeEventListener('keydown', onkeydown, true);
      removeEventListener('wheel', hide, true);
      removeEventListener('scroll', hide, true);
      removeEventListener('resize', hide);
      removeEventListener('blur', hide);
    };
  });

  function step(dir: 1 | -1) {
    const n = menu.items.length;
    for (let k = 1; k <= n; k++) {
      const i = (active + dir * k + n * 2) % n;
      if (actionable(i)) { active = i; return; }
    }
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') { e.preventDefault(); menu.hide(); }
    else if (e.key === 'ArrowDown') { e.preventDefault(); step(1); }
    else if (e.key === 'ArrowUp') { e.preventDefault(); step(-1); }
    else if (e.key === 'Home') { e.preventDefault(); active = -1; step(1); }
    else if (e.key === 'End') { e.preventDefault(); active = 0; step(-1); }
    else if ((e.key === 'Enter' || e.key === ' ') && actionable(active)) { e.preventDefault(); run(active); }
    else if (e.key === 'Tab') menu.hide();
  }

  function run(i: number) {
    const it = menu.items[i];
    if (!it || it === 'sep' || it.disabled) return;
    menu.hide();
    it.run();
  }
</script>

{#if menu.open}
  {#if menu.sheet}<div class="backdrop" aria-hidden="true"></div>{/if}
  <div
    bind:this={box}
    class="cm"
    class:sheet={menu.sheet}
    style={menu.sheet ? undefined : `left:${left}px;top:${top}px`}
    role="menu"
    tabindex="-1"
    oncontextmenu={(e) => e.preventDefault()}
  >
    {#each menu.items as it, i}
      {#if it === 'sep'}
        <hr />
      {:else}
        <button
          type="button"
          role="menuitem"
          class:danger={it.danger}
          class:on={i === active}
          disabled={it.disabled}
          onclick={() => run(i)}
          onpointerenter={() => actionable(i) && (active = i)}
        >
          <span class="ic" aria-hidden="true">{#if it.icon}<Icon name={it.icon} size={menu.sheet ? 19 : 16} />{/if}</span>
          <span class="lb">{it.label}</span>
        </button>
      {/if}
    {/each}
  </div>
{/if}

<style>
  .cm {
    position: fixed;
    z-index: 100;
    min-width: 200px;
    max-width: min(320px, calc(100vw - 16px));
    padding: 5px;
    background: var(--raised);
    border: 1px solid var(--rule);
    border-radius: 10px;
    box-shadow: var(--shadow);
    display: grid;
    outline: none;
    animation: pop 0.1s ease-out;
  }
  @keyframes pop {
    from {
      opacity: 0;
      transform: scale(0.97);
    }
  }
  button {
    display: flex;
    align-items: center;
    gap: 10px;
    text-align: left;
    padding: 7px 12px 7px 10px;
    border-radius: 6px;
    white-space: nowrap;
  }
  .ic {
    flex: none;
    width: 18px;
    display: inline-grid;
    place-items: center;
    color: var(--ink-3);
  }
  button.on .ic {
    color: var(--accent);
  }
  button.danger .ic {
    color: var(--bad);
  }
  button:disabled .ic {
    opacity: 0.5;
  }
  .lb {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  button.on {
    background: var(--accent-wash);
  }
  button.danger {
    color: var(--bad);
  }
  button:disabled {
    color: var(--ink-3);
    cursor: default;
  }
  hr {
    border: 0;
    border-top: 1px solid var(--rule);
    margin: 4px 6px;
  }
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 99;
    background: var(--scrim);
  }
  /* Touch: a bottom sheet with thumb-sized rows. */
  .cm.sheet {
    left: 0;
    right: 0;
    top: auto;
    bottom: 0;
    max-width: none;
    border-radius: 16px 16px 0 0;
    border-bottom: 0;
    padding: 8px 8px calc(10px + env(safe-area-inset-bottom));
    animation: rise 0.16s ease-out;
  }
  .sheet button {
    gap: 14px;
    padding: 13px 14px;
    font-size: 16px;
  }
  @keyframes rise {
    from {
      transform: translateY(24px);
      opacity: 0;
    }
  }
</style>
