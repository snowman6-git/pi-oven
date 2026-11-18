<script lang="ts">
  // Settings → Accounts: who is signed in to each plan provider, the other accounts pi-oven keeps, switching between
  // them, adding one (pi's own sign-in, run on the server; the page shows its text and passes the code back), and
  // Baton Touch (at a limit, another account takes over instead of waiting).
  import { t } from '../lib/i18n.svelte';
  import { api, type AccountInfo, type AccountsView, type BatonTarget } from '../lib/api';
  import { app } from '../lib/app.svelte';
  import { limits } from '../lib/limits.svelte';
  import { providerIcon } from '../lib/limits';
  import { clock } from '../lib/util';
  import Icon from './Icon.svelte';

  let view = $state<AccountsView | null>(null);
  let error = $state('');
  let busy = $state<string | null>(null);
  let code = $state('');

  // one question at a time: on a slow link the sign-in poll must not pile up requests behind a stalled one
  let loading = false;
  async function load() {
    if (loading) return;
    loading = true;
    try {
      view = await api.accounts();
      error = '';
    } catch (e) {
      error = (e as Error).message;
    } finally {
      loading = false;
    }
  }
  $effect(() => {
    void load();
  });
  // an account changed elsewhere (Baton Touch, another device): the list follows
  $effect(() => {
    if (limits.swap) setTimeout(() => void load(), 600);
  });
  // while a sign-in runs, its text comes in as it is printed
  const signing = $derived(view?.login?.state === 'running');
  $effect(() => {
    if (!signing) return;
    const id = setInterval(() => void load(), 1500);
    return () => clearInterval(id);
  });

  const NAMES: Record<string, string> = { anthropic: 'Claude', 'openai-codex': 'ChatGPT' };
  const label = (a: AccountInfo) => a.email || a.name || a.id.split(':')[1];
  const plan = (p?: string | null) => (p ? p.charAt(0).toUpperCase() + p.slice(1) : '');

  async function act(id: string, run: () => Promise<AccountsView>) {
    busy = id;
    try {
      view = await run();
      error = '';
    } catch (e) {
      error = (e as Error).message;
    } finally {
      busy = null;
    }
  }
  async function use(provider: string, a: AccountInfo) {
    await act(a.id, () => api.switchAccount(a.id));
    if (!error) limits.swapped(provider, label(a));
  }
  async function remove(a: AccountInfo) {
    const ok = await app.ask({ title: t('accounts.removeAsk', { who: label(a) }), body: t('accounts.removeBody'), confirm: t('accounts.remove'), danger: true });
    if (ok) await act(a.id, () => api.removeAccount(a.id));
  }
  // this server has no pseudo terminal: sign in on its terminal, then reload (the list picks the new sign-in up)
  let noTerminal = $state<string | null>(null);
  async function add(provider: string) {
    code = '';
    noTerminal = null;
    await act('login:' + provider, () => api.loginStart(provider));
    if (error.includes('no-terminal')) {
      noTerminal = provider;
      error = '';
    }
  }
  async function send() {
    const text = code.trim();
    if (!text) return;
    try {
      await api.loginInput(text);
      code = '';
      setTimeout(() => void load(), 800);
    } catch (e) {
      error = (e as Error).message;
    }
  }
  async function closeLogin() {
    try { await api.loginCancel(); } catch { /* already over */ }
    await load();
  }
  // other providers Baton Touch may hand over to: the providers pi knows (subscriptions and those in Settings → Models)
  const models = $derived(app.current?.models ?? []);
  const others = $derived([...new Set(models.map((m) => m.provider))]);
  const modelsOf = (p: string) => models.filter((m) => m.provider === p);
  const target = (p: string) => view?.targets.find((t) => t.provider === p);
  function setTargets(next: BatonTarget[]) {
    void act('baton', () => api.setBaton({ targets: next }));
  }
  function allow(p: string, on: boolean) {
    const now = view?.targets ?? [];
    setTargets(on ? [...now.filter((t) => t.provider !== p), { provider: p, model: modelsOf(p)[0]?.id ?? '' }] : now.filter((t) => t.provider !== p));
  }
  function pick(p: string, model: string) {
    setTargets((view?.targets ?? []).map((t) => (t.provider === p ? { ...t, model } : t)));
  }
  const NAME = (p: string) => NAMES[p] ?? p;

  // an address in the sign-in text becomes a link
  const parts = (line: string) => line.split(/(https:\/\/\S+)/g);
</script>

{#if view}
  <p class="note">{t('accounts.shared')}</p>
  {#each view.providers as p (p.provider)}
    {@const icon = providerIcon(p.provider)}
    <section>
      <h3 class="ph">
        {#if icon}<Icon name={icon} size={16} />{/if}{NAMES[p.provider] ?? p.provider}
      </h3>
      {#if p.accounts.length}
        <ul class="accts">
          {#each p.accounts as a (a.id)}
            <li class:cur={a.current}>
              <span class="who">
                <b data-priv="user">{label(a)}</b>
                <span class="tags">
                  {#if a.plan}<span class="tag">{plan(a.plan)}</span>{/if}
                  {#if a.current}<span class="tag on">{t('accounts.inUse')}</span>{/if}
                  {#if a.limitedUntil}<span class="lim">{t('accounts.limited', { at: clock(a.limitedUntil) })}</span>{/if}
                </span>
              </span>
              {#if !a.current}
                <button class="btn" type="button" disabled={busy !== null} onclick={() => use(p.provider, a)}>{t('accounts.use')}</button>
                <button class="btn quiet" type="button" disabled={busy !== null} aria-label={t('accounts.removeLabel', { who: label(a) })} onclick={() => remove(a)}><Icon name="trash" size={14} /></button>
              {/if}
            </li>
          {/each}
        </ul>
      {:else}
        <p class="note">{t('accounts.none')}</p>
      {/if}
      {#if view.login?.provider === p.provider}
        {@const l = view.login}
        <div class="login" class:bad={l.state === 'failed'}>
          {#if l.lines.length}
            <pre class="term" data-priv="secrets">{#each l.lines as line}{#each parts(line) as part}{#if part.startsWith('https://')}<a href={part} target="_blank" rel="noopener noreferrer">{part}</a>{:else}{part}{/if}{/each}
{/each}</pre>
          {:else if l.state === 'running'}
            <p class="note">{t('accounts.starting')}</p>
          {/if}
          {#if l.state === 'running'}
            <form class="code" onsubmit={(e) => { e.preventDefault(); void send(); }}>
              <input class="field mono" bind:value={code} placeholder={t('accounts.codePh')} autocomplete="off" spellcheck="false" aria-label={t('accounts.codeLabel')} />
              <button class="btn primary" type="submit" disabled={!code.trim()}>{t('accounts.send')}</button>
              <button class="btn quiet" type="button" onclick={closeLogin}>{t('accounts.cancel')}</button>
            </form>
          {:else}
            <p class="note" class:err={l.state === 'failed'}>{l.state === 'done' ? t('accounts.added') : t('accounts.failed', { why: l.error ?? '' })}</p>
            <button class="btn quiet" type="button" onclick={closeLogin}>{t('accounts.close')}</button>
          {/if}
        </div>
      {:else if noTerminal === p.provider}
        <div class="login">
          <p class="note">{t('accounts.noTerminal', { provider: p.provider })}</p>
          <button class="btn" type="button" onclick={() => { noTerminal = null; void load(); }}>{t('accounts.reload')}</button>
        </div>
      {:else}
        <button class="btn add" type="button" disabled={busy !== null || signing} onclick={() => add(p.provider)}><Icon name="plus" size={14} />{t('accounts.add')}</button>
      {/if}
    </section>
  {/each}
  <section>
    <h3>Baton Touch</h3>
    <label class="row">
      <span>{t('accounts.baton')}<small>{t('accounts.batonHint')}</small></span>
      <input class="switch" type="checkbox" role="switch" checked={view.batonTouch} onchange={(e) => { const on = e.currentTarget.checked; void act('baton', () => api.setBaton({ enabled: on })); }} />
    </label>
    <label class="row">
      <span>{t('accounts.cross')}<small>{t('accounts.crossHint')}</small></span>
      <input class="switch" type="checkbox" role="switch" disabled={!view.batonTouch} checked={view.crossProvider} onchange={(e) => { const on = e.currentTarget.checked; void act('baton', () => api.setBaton({ crossProvider: on })); }} />
    </label>
    {#if view.batonTouch && view.crossProvider}
      {#if others.length}
        <ul class="targets">
          {#each others as p (p)}
            {@const tg = target(p)}
            {@const icon = providerIcon(p)}
            <li>
              <label class="trow">
                <span class="tname">{#if icon}<Icon name={icon} size={14} />{/if}<span data-priv="hosts">{NAME(p)}</span>{#if tg}<span class="ord">{view.targets.indexOf(tg) + 1}</span>{/if}</span>
                <input class="switch" type="checkbox" role="switch" checked={!!tg} onchange={(e) => allow(p, e.currentTarget.checked)} />
              </label>
              {#if tg}
                <select class="field" aria-label={t('accounts.crossModel', { provider: NAME(p) })} value={tg.model} onchange={(e) => pick(p, e.currentTarget.value)}>
                  {#each modelsOf(p) as m (m.id)}<option value={m.id}>{m.name ?? m.id}</option>{/each}
                </select>
              {/if}
            </li>
          {/each}
        </ul>
        <p class="note">{t('accounts.crossOrder')}</p>
        <label class="row">
          <span>{t('accounts.home')}<small>{t('accounts.homeHint')}</small></span>
          <input class="switch" type="checkbox" role="switch" checked={view.returnHome} onchange={(e) => { const on = e.currentTarget.checked; void act('baton', () => api.setBaton({ returnHome: on })); }} />
        </label>
      {:else}
        <p class="note">{t('accounts.crossNoModels')}</p>
      {/if}
    {/if}
  </section>
{:else if !error}
  <p class="note">{t('accounts.loading')}</p>
{/if}
{#if error}<p class="note err" role="alert">{error}</p>{/if}

<style>
  /* the settings page's own look (its rules do not reach into this component) */
  h3 {
    margin: 0 0 8px;
    font-size: 13.5px;
    color: var(--ink-2);
  }
  .note {
    color: var(--ink-3);
    font-size: 12.5px;
    margin: 0;
  }
  .note.err {
    color: var(--bad);
  }
  .row {
    display: flex;
    justify-content: space-between;
    gap: 14px;
    align-items: center;
    padding: 6px 0;
    cursor: pointer;
  }
  .row > span {
    flex: 1;
    min-width: 0;
  }
  .row small {
    display: block;
    margin-top: 2px;
    color: var(--ink-3);
    font-size: 12.5px;
    line-height: 1.5;
  }
  .ph {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .accts {
    list-style: none;
    margin: 0 0 10px;
    padding: 0;
    display: flex;
    flex-direction: column;
    border: 1px solid var(--rule);
    border-radius: 10px;
    overflow: hidden;
  }
  .accts li {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 12px;
    min-width: 0;
  }
  .accts li + li {
    border-top: 1px solid var(--rule);
  }
  .who {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .who b {
    font-weight: 560;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tags {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
  }
  .tag {
    font-size: 11px;
    padding: 1px 7px;
    border-radius: 999px;
    background: var(--rule);
    color: var(--ink-2);
  }
  .tag.on {
    background: var(--accent-wash);
    color: var(--accent);
  }
  .lim {
    font-size: 12px;
    color: var(--heat);
  }
  .targets {
    list-style: none;
    margin: 4px 0 8px;
    padding: 0 0 0 14px;
    border-left: 2px solid var(--rule);
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .targets li {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .trow {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    padding: 4px 0;
    cursor: pointer;
  }
  .tname {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    min-width: 0;
    font-size: 13.5px;
  }
  .ord {
    font-size: 11px;
    padding: 0 6px;
    border-radius: 999px;
    background: var(--accent-wash);
    color: var(--accent);
    font-variant-numeric: tabular-nums;
  }
  .targets select {
    max-width: 100%;
  }
  .add {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .login {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 12px;
    border: 1px solid var(--rule);
    border-radius: 10px;
    background: var(--paper-sunk);
  }
  .login.bad {
    border-color: color-mix(in srgb, var(--bad) 45%, var(--rule));
  }
  .term {
    margin: 0;
    max-height: 220px;
    overflow: auto;
    font-family: var(--font-mono);
    font-size: 12px;
    line-height: 1.55;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    color: var(--ink-2);
  }
  .term a {
    color: var(--accent);
  }
  .code {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
  }
  .code .field {
    flex: 1 1 200px;
    min-width: 0;
  }
  .mono {
    font-family: var(--font-mono);
  }
</style>
