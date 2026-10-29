<script lang="ts">
  import { app } from '../lib/app.svelte';

  let dlg = $state<HTMLDialogElement>();
  const d = $derived(app.dialog);

  $effect(() => { if (d && dlg && !dlg.open) dlg.showModal(); });
  const MAX = 6;
</script>

{#if d}
  <!-- Escape or a click on the backdrop answers "no". Focus starts on Cancel so a stray Enter never deletes. -->
  <dialog
    bind:this={dlg}
    oncancel={(e) => { e.preventDefault(); d.resolve(false); }}
    onclick={(e) => { if (e.target === dlg) d.resolve(false); }}
    aria-labelledby="cf-title"
    aria-describedby="cf-body"
  >
    <div class="box">
      <h2 id="cf-title">{d.title}</h2>
      {#if d.body}<p id="cf-body">{d.body}</p>{/if}
      {#if d.items?.length}
        <ul>
          {#each d.items.slice(0, MAX) as it}<li>{it}</li>{/each}
          {#if d.items.length > MAX}<li class="more">외 {d.items.length - MAX}개</li>{/if}
        </ul>
      {/if}
      <div class="foot">
        <!-- svelte-ignore a11y_autofocus -->
        <button class="btn" type="button" autofocus onclick={() => d.resolve(false)}>취소</button>
        <button class="btn" class:danger={d.danger} class:primary={!d.danger} type="button" onclick={() => d.resolve(true)}>{d.confirm}</button>
      </div>
    </div>
  </dialog>
{/if}

<style>
  dialog {
    border: 1px solid var(--rule);
    border-radius: 14px;
    background: var(--raised);
    color: var(--ink);
    padding: 0;
    width: min(440px, calc(100vw - 24px));
    box-shadow: var(--shadow);
  }
  dialog::backdrop {
    background: var(--scrim);
  }
  .box {
    padding: 22px;
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 12px;
  }
  h2 {
    margin: 0;
    font-size: 18px;
    letter-spacing: -0.01em;
    overflow-wrap: anywhere;
  }
  p {
    margin: 0;
    color: var(--ink-2);
    font-size: 14px;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 8px 12px;
    border-radius: 8px;
    background: var(--code-bg);
    display: grid;
    gap: 4px;
    max-height: 32dvh;
    overflow: auto;
    font-size: 13.5px;
  }
  li {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .more {
    color: var(--ink-3);
  }
  .foot {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 4px;
  }
</style>
