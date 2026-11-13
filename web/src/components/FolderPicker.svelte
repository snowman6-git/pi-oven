<script lang="ts">
  import { t } from '../lib/i18n.svelte';
  import { api } from '../lib/api';
  import { app } from '../lib/app.svelte';
  import type { DirListing } from '../lib/protocol';
  import { S } from '../lib/strings';
  import Icon from './Icon.svelte';

  let dlg: HTMLDialogElement;
  let listing = $state<DirListing | null>(null);
  let hidden = $state(false);
  let error = $state<string | null>(null);
  let manual = $state('');
  /** The new folder's name while it is being typed (null: not making one). */
  let newName = $state<string | null>(null);

  /** The server's reason, in the page's language when it is the folder limit (`--root`). */
  const reason = (e: unknown) => {
    const m = (e as Error).message;
    return /outside the allowed folder/.test(m) ? t('picker.outside') : m;
  };

  async function go(path?: string) {
    try {
      listing = await api.browse(path, hidden);
      manual = listing.path;
      error = null;
    } catch (e) {
      error = reason(e);
    }
  }

  $effect(() => {
    dlg.showModal();
    // Typing a path is optional; don't pop the on-screen keyboard just because the dialog opened.
    (document.activeElement as HTMLElement | null)?.blur();
    void go(app.moveTarget?.cwd ?? app.chats.find((c) => c.runtime.id === app.currentId)?.runtime.cwd);
  });

  function startNew() {
    newName = newName == null ? '' : null;
    error = null;
  }

  async function makeFolder() {
    const name = (newName ?? '').trim();
    if (!listing || !name) return;
    try {
      listing = await api.mkdir(listing.path, name);
      manual = listing.path;
      newName = null;
      error = null;
    } catch (e) {
      error = reason(e);
    }
  }

  function close() {
    app.folderPicker = false;
    app.moveTarget = null;
  }
  function start() {
    if (!listing) return;
    const move = app.moveTarget;
    if (move) void app.moveSession(move, listing.path);
    else void app.openFolder(listing.path);
    close();
  }
</script>

<dialog bind:this={dlg} onclose={close} aria-labelledby="fp-title">
  <form method="dialog" class="box" onsubmit={(e) => e.preventDefault()}>
    <h2 id="fp-title">{app.moveTarget ? t('picker.moveTitle') : S.chooseFolder}</h2>
    <p class="help">{app.moveTarget ? t('picker.moveHelp') : S.chooseFolderHelp}</p>

    <div class="path">
      <button class="icon-btn" type="button" disabled={!listing?.parent} aria-label={S.parentFolder} title={S.parentFolder} onclick={() => listing?.parent && go(listing.parent)}>
        <Icon name="up" />
      </button>
      <input class="field mono" bind:value={manual} aria-label={t('picker.path')} spellcheck="false"
        onkeydown={(e) => { if (e.key === 'Enter' && !e.isComposing) { e.preventDefault(); void go(manual); } }} />
      <button class="icon-btn" type="button" disabled={!listing} aria-label={t('picker.newFolder')} title={t('picker.newFolder')} aria-expanded={newName != null} onclick={startNew}>
        <Icon name="folderPlus" />
      </button>
    </div>

    {#if newName != null}
      <div class="path">
        <!-- svelte-ignore a11y_autofocus -->
        <input class="field" bind:value={newName} placeholder={t('picker.newFolderName')} aria-label={t('picker.newFolderName')} spellcheck="false" autofocus maxlength="255"
          onkeydown={(e) => {
            if (e.isComposing) return;
            if (e.key === 'Enter') { e.preventDefault(); void makeFolder(); }
            if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); newName = null; }
          }} />
        <button class="btn" type="button" disabled={!newName.trim()} onclick={makeFolder}>{t('picker.create')}</button>
      </div>
    {/if}

    {#if error}<p class="err" role="alert">{error}</p>{/if}

    <ul class="dirs">
      {#each listing?.entries ?? [] as d (d.path)}
        <li><button type="button" onclick={() => go(d.path)}><Icon name="folder" size={16} />{d.name}</button></li>
      {:else}
        {#if listing}<li class="none">{S.noSubfolders}</li>{/if}
      {/each}
    </ul>

    <div class="foot">
      <label class="chk"><input type="checkbox" bind:checked={hidden} onchange={() => go(listing?.path)} />{S.showHidden}</label>
      <span class="grow"></span>
      <button class="btn quiet" type="button" onclick={() => dlg.close()}>{S.cancel}</button>
      <button class="btn primary" type="button" disabled={!listing} onclick={start}>
        {app.moveTarget ? t('picker.moveHere') : S.startHere}{#if listing?.isGit}<span class="git">git</span>{/if}
      </button>
    </div>
  </form>
</dialog>

<style>
  dialog {
    border: 1px solid var(--rule);
    border-radius: 14px;
    background: var(--raised);
    color: var(--ink);
    padding: 0;
    width: min(560px, calc(100vw - 24px));
    box-shadow: var(--shadow);
  }
  dialog::backdrop {
    background: var(--scrim);
  }
  .box {
    padding: 22px 22px 16px;
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 12px;
  }
  h2 {
    margin: 0;
    font-size: 20px;
    letter-spacing: -0.02em;
  }
  .help {
    margin: -6px 0 2px;
    color: var(--ink-2);
    font-size: 14px;
  }
  .path {
    display: flex;
    gap: 6px;
  }
  .path .field {
    flex: 1;
    min-width: 0;
  }
  .err {
    margin: 0;
    color: var(--bad);
    font-size: 13.5px;
  }
  .dirs {
    list-style: none;
    margin: 0;
    padding: 4px;
    height: 300px;
    overflow: auto;
    border: 1px solid var(--rule);
    border-radius: 10px;
    background: var(--paper);
  }
  .dirs button {
    width: 100%;
    overflow-wrap: anywhere;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px 10px;
    text-align: left;
    border-radius: 6px;
  }
  @media (hover: hover) {
    .dirs button:hover {
      background: var(--paper-sunk);
    }
  }

  .none {
    padding: 14px;
    color: var(--ink-3);
  }
  .foot {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .grow {
    flex: 1;
  }
  .chk {
    display: inline-flex;
    gap: 8px;
    align-items: center;
    font-size: 13.5px;
    color: var(--ink-2);
  }
  @media (max-width: 720px) {
    dialog {
      width: 100%;
      max-width: none;
      height: 100dvh;
      max-height: none;
      margin: 0;
      border: 0;
      border-radius: 0;
      box-shadow: none;
    }
    .box {
      display: flex;
      flex-direction: column;
      height: 100dvh;
      padding: calc(18px + env(safe-area-inset-top)) 16px calc(14px + env(safe-area-inset-bottom));
    }
    .dirs {
      flex: 1;
      height: auto;
      min-height: 0;
    }
  }
  .git {
    margin-left: 4px;
    font-size: 11px;
    padding: 0 6px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--accent-ink) 22%, transparent);
  }
</style>
