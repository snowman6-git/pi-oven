<script lang="ts">
  import { S } from '../lib/strings';
  import { t } from '../lib/i18n.svelte';
  import { api, HttpError, type ModelProvider, type ModelsConfig } from '../lib/api';
  import { app } from '../lib/app.svelte';
  import type { Chat } from '../lib/chat.svelte';
  import Icon from './Icon.svelte';

  // Settings → Models: what is in pi's models.json, and adding to it: an address, the style of API it speaks, a key, and
  // the model's id (this page never sees a saved key again, only a hint of it). The open session, if any, is told to
  // read the list again; sessions opened later read the file when they start.
  let { chat }: { chat?: Chat } = $props();
  const escapeHtml = (v: string) => v.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
  let cfg = $state<ModelsConfig | null>(null);
  let loadError = $state<string | null>(null);

  const APIS: Record<string, string> = {
    'openai-completions': '',
    'openai-responses': 'OpenAI Responses',
    'anthropic-messages': 'Anthropic Messages',
    'google-generative-ai': 'Google Generative AI',
  };

  let provider = $state('');
  let apiKind = $state('openai-completions');
  let baseUrl = $state('');
  let key = $state('');
  let clearKey = $state(false);
  let modelId = $state('');
  let modelName = $state('');
  let reasoning = $state(false);
  let image = $state(false);
  let ctxWin = $state('');
  let maxTok = $state('');

  let found = $state<string[] | null>(null);
  let probing = $state(false);
  let probeError = $state<string | null>(null);
  let saving = $state(false);
  let error = $state<string | null>(null);
  /** After a save: whether the open session can already see the model. */
  let applied = $state<null | { id: string; visible: boolean }>(null);

  const existing = $derived(cfg?.providers.find((p) => p.name === provider.trim()));
  const keyHint = $derived(existing?.key.kind === 'none' ? '' : existing?.key.hint);

  async function load() {
    try {
      cfg = await api.modelsConfig();
      loadError = null;
    } catch (e) {
      loadError = e instanceof HttpError && e.status === 422 ? e.message : t('models.loadFailed');
    }
  }

  $effect(() => {
    void load();
  });

  /** Pick a provider that is already there: its address and style fill in, the key stays where it is. */
  function edit(p: ModelProvider) {
    provider = p.name;
    baseUrl = p.baseUrl ?? '';
    apiKind = p.api && p.api in APIS ? p.api : 'openai-completions';
    key = '';
    clearKey = false;
    found = null;
    probeError = null;
    applied = null;
  }

  async function probe() {
    probeError = null;
    found = null;
    probing = true;
    try {
      found = (await api.probeModels({ baseUrl, apiKey: key || undefined, provider: existing?.name })).ids;
      if (!found.length) probeError = t('models.emptyList');
    } catch (e) {
      probeError = e instanceof Error ? e.message : t('models.connectFailed');
    } finally {
      probing = false;
    }
  }

  const num = (v: string) => (v.trim() ? Number(v) : undefined);

  async function save() {
    error = null;
    if (!provider.trim() || !baseUrl.trim()) {
      error = t('models.needProvider');
      return;
    }
    if (!modelId.trim() && !existing) {
      error = t('models.needModel');
      return;
    }
    saving = true;
    try {
      await api.saveModel({
        provider: provider.trim(),
        baseUrl: baseUrl.trim(),
        api: apiKind,
        apiKey: clearKey ? '' : key ? key : undefined,
        model: modelId.trim() ? { id: modelId.trim(), name: modelName.trim() || undefined, reasoning, image, contextWindow: num(ctxWin), maxTokens: num(maxTok) } : undefined,
      });
      const id = modelId.trim();
      key = '';
      clearKey = false;
      await load();
      if (id) {
        const list = chat ? await chat.reloadModels() : null;
        applied = { id, visible: !list || list.some((m) => m.id === id && m.provider === provider.trim()) };
        modelId = modelName = ctxWin = maxTok = '';
        reasoning = image = false;
        found = null;
      } else app.flash(t('models.saved'));
    } catch (e) {
      error = e instanceof Error ? e.message : t('models.saveFailed');
    } finally {
      saving = false;
    }
  }

  let doneEl = $state<HTMLElement>();
  $effect(() => {
    if (applied) doneEl?.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
  });

  async function remove(p: ModelProvider, id?: string) {
    const ok = await app.ask({
      title: id ? t('models.delModelAsk') : t('models.delProviderAsk'),
      body: id ? t('models.delModelBody', { provider: p.name, id }) : t('models.delProviderBody', { provider: p.name, n: p.models.length }),
      confirm: t('models.remove'),
      danger: true,
    });
    if (!ok) return;
    try {
      await api.removeModel(p.name, id);
      await load();
      await chat?.reloadModels();
      app.flash(t('models.removed'));
    } catch {
      error = t('models.removeFailed');
    }
  }

  async function reopen() {
    if (!chat) return;
    app.settingsOpen = false;
    await app.restartChat(chat);
  }
</script>

<div class="box">
    {#if loadError}<p class="err" role="alert">{loadError}</p>{/if}

    {#if cfg && cfg.providers.length}
      <section class="have">
        <h3>{t('models.providers')}</h3>
        <ul>
          {#each cfg.providers as p (p.name)}
            <li class:on={p.name === provider.trim()}>
              <div class="top">
                <button class="pick" type="button" onclick={() => edit(p)} title={t('models.editHint')}>
                  <b>{p.name}</b>
                  <span class="mono" data-priv="hosts">{p.baseUrl ?? t('models.noAddress')}</span>
                </button>
                <button class="icon-btn sm" type="button" aria-label={t('models.removeName', { name: p.name })} title={t('models.removeProvider')} onclick={() => remove(p)}><Icon name="trash" size={15} /></button>
              </div>
              <div class="models">
                {#each p.models as m (m.id)}
                  <span class="mchip">{m.name || m.id}<button type="button" aria-label={t('models.removeName', { name: m.id })} onclick={() => remove(p, m.id)}>×</button></span>
                {:else}
                  <small>{t('models.noModels')}</small>
                {/each}
                {#if p.key.kind !== 'none'}<small class="khint" data-priv="secrets">{t('models.keyHint', { hint: p.key.hint ?? '' })}</small>{/if}
              </div>
            </li>
          {/each}
        </ul>
      </section>
    {/if}

    <form onsubmit={(e) => { e.preventDefault(); void save(); }}>
      <h3>{existing ? t('models.addTo', { name: existing.name }) : t('models.newProvider')}</h3>

      <label>{t('models.providerName')}
        <input class="field" bind:value={provider} placeholder={t('models.providerPh')} list="md-providers" autocomplete="off" spellcheck="false" />
        <datalist id="md-providers">{#each cfg?.providers ?? [] as p (p.name)}<option value={p.name}></option>{/each}</datalist>
      </label>

      <label>{t('models.api')}
        <select class="field" bind:value={apiKind}>
          {#each cfg?.apis ?? Object.keys(APIS) as a (a)}<option value={a}>{a === 'openai-completions' ? t('models.openaiCompat') : (APIS[a] ?? a)}</option>{/each}
        </select>
      </label>

      <label>{t('models.endpoint')}
        <input class="field mono" data-priv="hosts" bind:value={baseUrl} placeholder="http://localhost:8000/v1" autocomplete="off" spellcheck="false" />
      </label>

      <label>{t('models.apiKey')}
        <input class="field mono" data-priv="secrets" type="password" bind:value={key} autocomplete="new-password"
          placeholder={existing ? (keyHint ? t('models.keySaved', { hint: keyHint }) : t('models.keyNone')) : t('models.keyLocal')} />
        <small>{@html t('models.keyNote')}</small>
        {#if existing && keyHint}<span class="inline"><input type="checkbox" bind:checked={clearKey} id="md-clear" /><label for="md-clear" class="plain">{t('models.clearKey')}</label></span>{/if}
      </label>

      <div class="probe">
        <button class="btn" type="button" disabled={probing || !baseUrl.trim()} onclick={probe}>{probing ? S.loading : t('models.probe')}</button>
        {#if probeError}<small class="err" role="alert">{probeError}</small>{/if}
      </div>
      {#if found?.length}
        <div class="found" role="group" aria-label={t('models.found')}>
          {#each found as id (id)}
            <button type="button" class="fchip" class:on={id === modelId} onclick={() => { modelId = id; if (!modelName) modelName = ''; }}>{id}</button>
          {/each}
        </div>
      {/if}

      <label>{t('models.modelId')}
        <input class="field mono" bind:value={modelId} placeholder={t('models.modelIdPh')} autocomplete="off" spellcheck="false" />
      </label>
      <label><span>{t('models.displayName')} <small>{t('models.optional')}</small></span>
        <input class="field" bind:value={modelName} placeholder={t('models.displayPh')} autocomplete="off" />
      </label>

      <div class="checks">
        <label class="inline"><input type="checkbox" bind:checked={reasoning} /> {t('models.reasoning')}</label>
        <label class="inline"><input type="checkbox" bind:checked={image} /> {t('models.image')}</label>
      </div>
      <div class="nums">
        <label><span>{t('models.ctxWin')} <small>{t('models.tokens')}</small></span><input class="field" inputmode="numeric" bind:value={ctxWin} placeholder="128000" /></label>
        <label><span>{t('models.maxOut')} <small>{t('models.tokens')}</small></span><input class="field" inputmode="numeric" bind:value={maxTok} placeholder="16384" /></label>
      </div>

      {#if error}<p class="err" role="alert">{error}</p>{/if}
      {#if applied}
        <p class="done" role="status" bind:this={doneEl}>
          {@html t('models.added', { id: escapeHtml(applied.id) })}
          {#if !chat}{t('models.newSessions')}
          {:else if applied.visible}{t('models.visible')}
          {:else}{t('models.notVisible')}
            <button class="btn" type="button" onclick={reopen}>{S.restart}</button>{/if}
        </p>
      {/if}

      <div class="actions">
        <button class="btn primary" type="submit" disabled={saving}>{saving ? t('models.saving') : modelId.trim() ? t('models.addModel') : t('models.saveProvider')}</button>
      </div>
      {#if cfg}<p class="path mono" data-priv="paths">{cfg.path}</p>{/if}
    </form>
</div>

<style>
  .box { display: grid; grid-template-columns: minmax(0, 1fr); gap: 14px; margin-top: 14px; }
  h3 { margin: 0 0 4px; font-size: 13px; font-weight: 700; color: var(--ink-2); }
  form { display: grid; gap: 12px; min-width: 0; }
  label { display: grid; gap: 5px; font-size: 13px; font-weight: 600; color: var(--ink-2); min-width: 0; }
  label small, .khint { font-weight: 450; color: var(--ink-3); font-size: 12px; }
  label small :global(code) { font-size: 11.5px; }
  .field { height: 36px; width: 100%; box-sizing: border-box; font-weight: 450; color: var(--ink); }
  select.field { padding: 0 10px; font: inherit; font-size: 14px; text-overflow: ellipsis; background: var(--paper); border: 1px solid var(--rule); border-radius: 8px; }
  .inline { display: flex; align-items: center; gap: 8px; font-weight: 500; }
  label.plain { font-weight: 450; display: inline; }
  .checks { display: flex; flex-wrap: wrap; gap: 8px 22px; }
  .nums { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 12px; }
  .probe { display: flex; align-items: center; flex-wrap: wrap; gap: 10px; }
  .found { display: flex; flex-wrap: wrap; gap: 6px; max-height: 128px; overflow: auto; scrollbar-width: none; }
  .fchip { padding: 4px 10px; border-radius: 999px; border: 1px solid var(--rule); background: var(--paper); font: 12.5px var(--font-mono); color: var(--ink-2); }
  .fchip.on { border-color: var(--accent); color: var(--accent); background: var(--accent-wash); }
  .err { margin: 0; color: var(--bad); font-size: 13px; overflow-wrap: anywhere; }
  .done { margin: 0; padding: 10px 12px; border-radius: 8px; background: var(--paper-sunk); border: 1px solid color-mix(in srgb, var(--ok) 40%, var(--rule)); font-size: 13.5px; line-height: 1.6; display: flex; flex-wrap: wrap; gap: 4px 10px; align-items: center; }
  .actions { display: flex; justify-content: flex-end; }
  .path { margin: 0; font-size: 11.5px; color: var(--ink-3); overflow-wrap: anywhere; }
  .have ul { list-style: none; margin: 0; padding: 0; display: grid; grid-template-columns: minmax(0, 1fr); gap: 6px; }
  .have li { border: 1px solid var(--rule); border-radius: 10px; background: var(--paper); padding: 8px 10px; display: grid; gap: 6px; }
  .have li.on { border-color: var(--ink-3); }
  .top { display: flex; align-items: center; gap: 8px; }
  .pick { flex: 1; min-width: 0; display: grid; gap: 1px; text-align: left; }
  .pick span { font-size: 12px; color: var(--ink-3); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .sm { width: 28px; height: 28px; }
  .models { display: flex; flex-wrap: wrap; gap: 5px; align-items: center; }
  .mchip { display: inline-flex; align-items: center; gap: 4px; padding: 2px 4px 2px 10px; border-radius: 999px; background: var(--paper-sunk); font-size: 12.5px; }
  .mchip button { width: 18px; height: 18px; border-radius: 50%; color: var(--ink-3); line-height: 1; }
  @media (hover: hover) { .mchip button:hover { background: var(--rule); color: var(--ink); } }
  @media (max-width: 720px) {
    .nums { grid-template-columns: minmax(0, 1fr); }
  }
</style>
