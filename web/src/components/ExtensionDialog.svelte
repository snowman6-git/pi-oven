<script lang="ts">
  import type { Chat } from '../lib/chat.svelte';
  import { S } from '../lib/strings';
  import { autofocus } from '../lib/actions';

  let { chat }: { chat: Chat } = $props();

  const d = $derived(chat.dialogs[0]);
  let dlg = $state<HTMLDialogElement>();
  let value = $state('');

  // Re-seed the field whenever a different request is shown.
  $effect(() => {
    if (!d) return;
    value = d.method === 'editor' ? (d.prefill ?? '') : '';
    if (dlg && !dlg.open) dlg.showModal();
  });

  const cancel = () => d && chat.respond(d.id, { cancelled: true });
</script>

{#if d}
  {#key d.id}
    <dialog bind:this={dlg} oncancel={(e) => { e.preventDefault(); cancel(); }} aria-labelledby="xd-title">
      <div class="box">
        <h2 id="xd-title">{d.title}</h2>

        {#if d.method === 'confirm'}
          <p class="msg">{d.message}</p>
          <div class="foot">
            <button class="btn" type="button" onclick={() => chat.respond(d.id, { confirmed: false })}>{S.cancel}</button>
            <button class="btn primary" type="button" use:autofocus onclick={() => chat.respond(d.id, { confirmed: true })}>{S.confirm}</button>
          </div>
        {:else if d.method === 'select'}
          <ul class="opts">
            {#each d.options as o, i}
              <li><button type="button" use:autofocus={i === 0} onclick={() => chat.respond(d.id, { value: o })}>{o}</button></li>
            {/each}
          </ul>
          <div class="foot"><button class="btn quiet" type="button" onclick={cancel}>{S.cancel}</button></div>
        {:else}
          {#if d.method === 'input'}
            <input class="field" bind:value placeholder={d.placeholder ?? ''} use:autofocus
              onkeydown={(e) => { if (e.key === 'Enter' && !e.isComposing) chat.respond(d.id, { value }); }} />
          {:else}
            <textarea class="field mono" rows="12" bind:value use:autofocus></textarea>
          {/if}
          <div class="foot">
            <button class="btn quiet" type="button" onclick={cancel}>{S.cancel}</button>
            <button class="btn primary" type="button" onclick={() => chat.respond(d.id, { value })}>{S.submit}</button>
          </div>
        {/if}
      </div>
    </dialog>
  {/key}
{/if}

<style>
  dialog {
    border: 1px solid var(--rule);
    border-radius: 14px;
    background: var(--raised);
    color: var(--ink);
    padding: 0;
    width: min(520px, calc(100vw - 24px));
    box-shadow: var(--shadow);
  }
  dialog::backdrop {
    background: var(--scrim);
  }
  .box {
    padding: 22px;
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 14px;
  }
  h2 {
    margin: 0;
    font-size: 18px;
    letter-spacing: -0.01em;
    overflow-wrap: anywhere;
  }
  .msg {
    margin: 0;
    padding: 10px 12px;
    border-radius: 8px;
    background: var(--code-bg);
    color: var(--ink-2);
    font: 13px/1.55 var(--font-mono);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    max-height: 40dvh;
    overflow: auto;
  }
  .opts {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 4px;
    max-height: 50vh;
    overflow: auto;
  }
  .opts button {
    width: 100%;
    overflow-wrap: anywhere;
    text-align: left;
    padding: 9px 12px;
    border-radius: 8px;
    border: 1px solid var(--rule);
  }
  @media (hover: hover) {
    .opts button:hover {
      border-color: var(--accent);
      background: var(--accent-wash);
    }
  }

  textarea.field {
    height: auto;
    padding: 10px;
    resize: vertical;
  }
  .foot {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
</style>
