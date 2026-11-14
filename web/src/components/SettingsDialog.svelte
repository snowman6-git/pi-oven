<script lang="ts">
  import { app } from '../lib/app.svelte';
  import { hasMaple } from '../lib/maple';
  import { pinRules } from '../lib/pinrules.svelte';
  import { PARTS, showcase } from '../lib/showcase.svelte';
  import { S } from '../lib/strings';
  import { api, auth, type LongMode, type ResumeSettings } from '../lib/api';
  import { i18n, LANGS, setLang, t } from '../lib/i18n.svelte';
  import About from './About.svelte';
  import ModelManager from './ModelManager.svelte';
  import type { IconName } from '../lib/icons';
  import Icon from './Icon.svelte';
  import { serverUrl, settings, type FontSize, type StreamFx, type ToolImages } from '../lib/settings.svelte';

  let dlg: HTMLDialogElement;
  let panel = $state<HTMLElement>();

  // One long page was hard to find things in: the settings are split by what they are about.
  type Tab = 'view' | 'chat' | 'pins' | 'models' | 'showcase' | 'server' | 'about';
  const TABS: { id: Tab; icon: IconName }[] = [
    { id: 'view', icon: 'light' },
    { id: 'chat', icon: 'chat' },
    { id: 'pins', icon: 'pin' },
    { id: 'models', icon: 'code' },
    { id: 'showcase', icon: 'eye' },
    { id: 'server', icon: 'link' },
    { id: 'about', icon: 'info' },
  ];
  // kept by the server, not this browser: the schedule runs there, for every device
  let longMode = $state<LongMode | null>(null);
  let longError = $state('');
  $effect(() => { api.longMode().then((m) => (longMode = m)).catch((e) => (longError = (e as Error).message)); });
  let pollOn = $state<boolean | null>(null);
  $effect(() => { api.limitsPoll().then((p) => (pollOn = p.enabled)).catch(() => {}); });
  async function setPoll(on: boolean) {
    try { pollOn = (await api.setLimitsPoll(on)).enabled; } catch (e) { resumeError = (e as Error).message; }
  }
  let resumeSet = $state<ResumeSettings | null>(null);
  let resumeText = $state('');
  let resumeError = $state('');
  $effect(() => { api.resumeSettings().then((s) => ((resumeSet = s), (resumeText = s.text))).catch((e) => (resumeError = (e as Error).message)); });
  async function saveResume(enabled: boolean, text: string) {
    resumeError = '';
    const clean = text.trim() || t('resume.default');
    try { resumeSet = await api.setResumeSettings({ enabled, text: clean }); resumeText = resumeSet.text; }
    catch (e) { resumeError = (e as Error).message; }
  }
  async function toggleLong(box: HTMLInputElement) {
    longError = '';
    try { longMode = await api.setLongMode(box.checked); }
    catch (e) { box.checked = !box.checked; longError = (e as Error).message; }
  }


  const TAB_KEY = 'pi-oven.settings-tab';
  let tab = $state<Tab>(((): Tab => {
    const asked = app.settingsTab;
    app.settingsTab = null;
    if (TABS.some((x) => x.id === asked)) return asked as Tab;
    try { const v = localStorage.getItem(TAB_KEY); return TABS.some((x) => x.id === v) ? (v as Tab) : 'view'; } catch { return 'view'; }
  })());
  function pick(id: Tab) {
    tab = id;
    try { localStorage.setItem(TAB_KEY, id); } catch { /* private mode */ }
    panel?.scrollTo({ top: 0 });
  }
  /** Arrow keys move between tabs (Home/End to the ends), and focus follows. */
  function onTabKey(e: KeyboardEvent) {
    const i = TABS.findIndex((x) => x.id === tab);
    const step = { ArrowDown: 1, ArrowRight: 1, ArrowUp: -1, ArrowLeft: -1 }[e.key];
    const next = step ? (i + step + TABS.length) % TABS.length : e.key === 'Home' ? 0 : e.key === 'End' ? TABS.length - 1 : -1;
    if (next < 0) return;
    e.preventDefault();
    pick(TABS[next].id);
    queueMicrotask(() => document.getElementById(`st-tab-${TABS[next].id}`)?.focus());
  }
  let server = $state(serverUrl.value);
  let replay = $state(0);

  $effect(() => dlg.showModal());

  const themes = ['dark', 'light', 'auto'] as const;
  const toolImgs: ToolImages[] = ['always', 'expanded'];
  const fonts: FontSize[] = ['small', 'normal', 'large'];
  const fxs: StreamFx[] = ['bounce', 'fade', 'ember', 'none'];

  function pickFx(id: StreamFx) {
    settings.set('fx', id);
    replay++;
  }

  let ruleName = $state('');
  let rulePattern = $state('');
  let ruleError = $state<string | null>(null);
  function addRule() {
    ruleError = pinRules.add(ruleName, rulePattern);
    if (!ruleError) ruleName = rulePattern = '';
  }

  // An address nothing answers on is not applied: the page would only come back to the "cannot connect" screen.
  async function saveServer() {
    const next = server.trim().replace(/\/+$/, '');
    if (next === serverUrl.value) return;
    if (next) {
      try {
        const c = new AbortController();
        const timer = setTimeout(() => c.abort(), 5000);
        await fetch(next, { mode: 'no-cors', signal: c.signal, cache: 'no-store' });
        clearTimeout(timer);
      } catch {
        app.flash(S.serverUnreachable);
        return;
      }
    }
    serverUrl.value = next;
    location.reload();
  }

  // What "notify" can actually do here: desktop notifications need a secure page and the browser's permission;
  // without them only the tab title and a tone are left, and the switch should say so.
  const canNotify = typeof Notification !== 'undefined' && window.isSecureContext;
  let notifyPerm = $state(canNotify ? Notification.permission : 'denied');
  async function toggleNotify() {
    app.toggleNotify();
    if (app.notifyOn && canNotify && Notification.permission === 'default') notifyPerm = await Notification.requestPermission();
  }

  // Only the app has another way to pick a server: its first screen. There the field below would be a second one.
  const back = (window as unknown as { __piOvenBack?: () => void }).__piOvenBack;
  async function signOut() {
    const ok = await app.ask({ title: t('settings.server.signOutAsk'), body: t('settings.server.signOutBody'), confirm: t('settings.server.signOut'), danger: true });
    if (!ok) return;
    auth.token = null;
    location.reload();
  }
</script>

<dialog bind:this={dlg} onclose={() => (app.settingsOpen = false)} aria-labelledby="st-title">
  <div class="box">
    <header>
      <h2 id="st-title">{t('settings.title')}</h2>
      <button class="btn quiet" type="button" onclick={() => dlg.close()}>{S.close}</button>
    </header>

    <div class="tabs" role="tablist" aria-label={t('settings.tabsLabel')} aria-orientation={app.narrow ? 'horizontal' : 'vertical'}>
      {#each TABS as tb (tb.id)}
        <button type="button" role="tab" id="st-tab-{tb.id}" aria-controls="st-panel" aria-selected={tab === tb.id} tabindex={tab === tb.id ? 0 : -1}
          class:on={tab === tb.id} onclick={() => pick(tb.id)} onkeydown={onTabKey}>
          <Icon name={tb.icon} size={16} />{t(`settings.tab.${tb.id}`)}
        </button>
      {/each}
    </div>

    <div class="panel" id="st-panel" role="tabpanel" aria-labelledby="st-tab-{tab}" bind:this={panel}>
      {#if tab === 'view'}
      <section>
        <h3>{t('settings.language')}</h3>
        <div class="seg" role="radiogroup" aria-label={t('settings.language')}>
          {#each LANGS as l (l.code)}
            <button type="button" role="radio" lang={l.code} aria-checked={i18n.lang === l.code} class:on={i18n.lang === l.code} onclick={() => setLang(l.code)}>{l.name}</button>
          {/each}
        </div>
        <p class="note">{t('settings.languageHint')}</p>
      </section>
      <section>
        <h3>{S.theme}</h3>
        <div class="seg" role="radiogroup" aria-label={S.theme}>
          {#each themes as id}
            <button type="button" role="radio" aria-checked={app.theme === id} class:on={app.theme === id} onclick={() => app.setTheme(id)}>{t(`settings.theme.${id}`)}</button>
          {/each}
        </div>
      </section>
      <section>
        <h3>{t('settings.font.title')}</h3>
        <div class="seg" role="radiogroup" aria-label={t('settings.font.title')}>
          {#if hasMaple}<button type="button" role="radio" aria-checked={settings.family === 'maple'} class:on={settings.family === 'maple'} onclick={() => settings.set('family', 'maple')} style="font-family: 'NxMaple', var(--font-ui)">{t('settings.font.maple')}</button>{/if}
          <button type="button" role="radio" aria-checked={settings.family === 'system'} class:on={settings.family === 'system'} onclick={() => settings.set('family', 'system')} style="font-family: system-ui, -apple-system, 'Noto Sans KR', sans-serif">{t('settings.font.system')}</button>
        </div>
      </section>
      <section>
        <h3>{t('settings.font.size')}</h3>
        <div class="seg" role="radiogroup" aria-label={t('settings.font.size')}>
          {#each fonts as id}
            <button type="button" role="radio" aria-checked={settings.font === id} class:on={settings.font === id} onclick={() => settings.set('font', id)}>{t(`settings.font.${id}`)}</button>
          {/each}
        </div>
      </section>
      <section>
        <h3>{t('settings.fx.title')}</h3>
        <div class="fx" role="radiogroup" aria-label={t('settings.fx.title')}>
          {#each fxs as f (f)}
            <button type="button" role="radio" aria-checked={settings.fx === f} class:on={settings.fx === f} onclick={() => pickFx(f)}>
              <b>{t(`settings.fx.${f}.name`)}</b><small>{t(`settings.fx.${f}.hint`)}</small>
            </button>
          {/each}
        </div>
        {#key replay}
          <p class="sample md" data-fx={settings.fx} aria-hidden="true">
            {#each t('settings.fx.sample').split(' ') as w, i}
              <span class="w" style="animation-delay:{i * 90}ms">{w}</span>{' '}
            {/each}
          </p>
        {/key}
      </section>
      {:else if tab === 'chat'}
      <section>
        <h3>{t('settings.toolImg.title')}</h3>
        <div class="seg" role="radiogroup" aria-label={t('settings.toolImg.title')}>
          {#each toolImgs as id}
            <button type="button" role="radio" aria-checked={settings.toolImages === id} class:on={settings.toolImages === id} onclick={() => settings.set('toolImages', id)}>{t(`settings.toolImg.${id}`)}</button>
          {/each}
        </div>
      </section>
      <section>
        <h3>{t('settings.thinkNotify')}</h3>
        <label class="row">
          <span>{t('settings.thinkingOpen')}</span>
          <input class="switch" type="checkbox" role="switch" checked={settings.thinkingOpen} onchange={(e) => settings.set('thinkingOpen', e.currentTarget.checked)} />
        </label>
        <label class="row">
          <span>{t('settings.notify')}</span>
          <input class="switch" type="checkbox" role="switch" checked={app.notifyOn} onchange={toggleNotify} />
        </label>
        {#if app.notifyOn && notifyPerm !== 'granted'}<p class="note">{canNotify ? t('settings.notifyBlocked') : t('settings.notifyInsecure')}</p>{/if}
      </section>
      <section>
        <h3>{t('settings.meta.title')}</h3>
        <label class="row">
          <span>{t('settings.meta.context')}</span>
          <input class="switch" type="checkbox" role="switch" checked={settings.metaContext} onchange={(e) => settings.set('metaContext', e.currentTarget.checked)} />
        </label>
        <label class="row">
          <span>{t('settings.meta.limits')}<small>{t('settings.meta.limitsHint')}</small></span>
          <input class="switch" type="checkbox" role="switch" checked={settings.metaLimits} onchange={(e) => settings.set('metaLimits', e.currentTarget.checked)} />
        </label>
        <label class="row">
          <span>{t('settings.meta.limitText')}</span>
          <input class="switch" type="checkbox" role="switch" checked={settings.metaLimitText} disabled={!settings.metaLimits} onchange={(e) => settings.set('metaLimitText', e.currentTarget.checked)} />
        </label>
      </section>
      <section>
        <h3>{t('settings.retry.title')}</h3>
        <label class="row">
          <span>{t('long.name')}<small>{t('settings.retry.hint')}</small></span>
          <input class="switch" type="checkbox" role="switch" disabled={!longMode?.available} checked={longMode?.enabled ?? false} onchange={(e) => toggleLong(e.currentTarget)} />
        </label>
        {#if longMode && !longMode.available}<p class="note">{t('settings.retry.unavailable')}</p>{/if}
        {#if longError}<p class="note err">{longError}</p>{/if}
      </section>
      <section>
        <h3>{t('resume.title')}</h3>
        {#if pollOn != null}
          <label class="row">
            <span>{t('limits.pollName')}<small>{t('limits.pollHint')}</small></span>
            <input class="switch" type="checkbox" role="switch" checked={pollOn} onchange={(e) => setPoll(e.currentTarget.checked)} />
          </label>
        {/if}
        <label class="row">
          <span>{t('resume.name')}<small>{t('resume.hint')}</small></span>
          <input class="switch" type="checkbox" role="switch" disabled={!resumeSet} checked={resumeSet?.enabled ?? false} onchange={(e) => saveResume(e.currentTarget.checked, resumeText)} />
        </label>
        <label class="textrow">
          <span>{t('resume.textLabel')}</span>
          <input class="field" type="text" maxlength="2000" disabled={!resumeSet?.enabled} bind:value={resumeText} placeholder={t('resume.default')}
            onchange={() => resumeSet && saveResume(resumeSet.enabled, resumeText)}
            onkeydown={(e) => { if (e.key === 'Enter' && !e.isComposing) (e.currentTarget as HTMLInputElement).blur(); }} />
        </label>
        {#if resumeError}<p class="note err">{resumeError}</p>{/if}
      </section>
      {:else if tab === 'pins'}
      <section>
        <h3>{t('settings.pins.title')}</h3>
        <p class="note">{@html t('settings.pins.note')}</p>
        {#if pinRules.list.length}
          <ul class="rules">
            {#each pinRules.list as r (r.id)}
              <li>
                <label class="row"><span><b>{r.name}</b><small class="mono">{r.pattern}</small></span><input class="switch" type="checkbox" role="switch" checked={r.on} onchange={(e) => pinRules.toggle(r.id, e.currentTarget.checked)} /></label>
                <button class="btn quiet" type="button" aria-label={t('settings.pins.removeLabel', { name: r.name })} onclick={() => pinRules.remove(r.id)}>{t('settings.pins.remove')}</button>
              </li>
            {/each}
          </ul>
        {/if}
        <form class="newrule" onsubmit={(e) => { e.preventDefault(); addRule(); }}>
          <input class="field" bind:value={ruleName} placeholder={t('settings.pins.name')} maxlength="40" aria-label={t('settings.pins.nameLabel')} />
          <input class="field mono" bind:value={rulePattern} placeholder={t('settings.pins.pattern')} spellcheck="false" autocomplete="off" aria-label={t('settings.pins.regex')} />
          <button class="btn" type="submit">{t('settings.pins.add')}</button>
        </form>
        {#if ruleError}<p class="note err" role="alert">{ruleError}</p>{/if}
      </section>
      {:else if tab === 'models'}
      <section>
        <h3>{t('settings.models.title')}</h3>
        <p class="note">{t('settings.models.note')}</p>
        <ModelManager chat={app.current} />
      </section>
      {:else if tab === 'showcase'}
      <section>
        <h3>{t('settings.showcase.title')}</h3>
        <p class="note">{t('settings.showcase.note')}</p>
        <label class="row">
          <span><b>{t('settings.showcase.on')}</b></span>
          <input class="switch" type="checkbox" role="switch" checked={showcase.on} onchange={(e) => showcase.setOn(e.currentTarget.checked)} />
        </label>
        <div class="seg" role="radiogroup" aria-label={t('settings.showcase.style')} class:off={!showcase.on}>
          <button type="button" role="radio" aria-checked={showcase.style === 'example'} class:on={showcase.style === 'example'} disabled={!showcase.on} onclick={() => showcase.setStyle('example')}>{t('settings.showcase.example')}</button>
          <button type="button" role="radio" aria-checked={showcase.style === 'blur'} class:on={showcase.style === 'blur'} disabled={!showcase.on} onclick={() => showcase.setStyle('blur')}>{t('settings.showcase.blur')}</button>
        </div>
        <div class="subs" class:off={!showcase.on} role="group" aria-label={t('settings.showcase.parts')}>
          {#each PARTS as p (p.id)}
            <label class="row sub">
              <span>{t(`showcase.${p.id}.label`)}<small>{t(`showcase.${p.id}.hint`)}</small></span>
              <input class="switch" type="checkbox" role="switch" checked={showcase.parts[p.id]} disabled={!showcase.on} onchange={(e) => showcase.set(p.id, e.currentTarget.checked)} />
            </label>
          {/each}
        </div>
      </section>
      {:else if tab === 'server'}
      <section>
        <h3>{t('settings.server.current')}</h3>
        <p class="cur"><span class="mono" data-priv="hosts">{showcase.text(serverUrl.value || location.origin)}</span>
          <small>{serverUrl.value ? '' : t('settings.server.thisPage')}{auth.token ? `${serverUrl.value ? '' : ' · '}${t('settings.server.signedIn')}` : ''}</small></p>
        {#if auth.token}<button class="btn" type="button" onclick={signOut}>{t('settings.server.signOut')}</button>{/if}
      </section>
      <section>
        <h3>{t('settings.server.switchTo')}</h3>
        {#if back}
          <p class="note">{t('settings.server.appNote')}</p>
          <button class="btn" type="button" onclick={back}>{t('settings.server.pick')}</button>
        {:else}
          <p class="note">{t('settings.server.note')}</p>
          <div class="srv">
            <input class="field mono" data-priv="hosts" bind:value={server} placeholder={serverUrl.value || location.origin} spellcheck="false" aria-label={t('settings.server.title')}
              onkeydown={(e) => e.key === 'Enter' && saveServer()} />
            <button class="btn" type="button" onclick={saveServer}>{t('settings.server.apply')}</button>
          </div>
          {#if serverUrl.value}<button class="btn quiet" type="button" onclick={() => ((server = ''), saveServer())}>{S.serverReset}</button>{/if}
        {/if}
      </section>
      {:else if tab === 'about'}
      <About />
      {/if}
    </div>
  </div>
</dialog>

<style>
  dialog {
    border: 1px solid var(--rule);
    border-radius: 14px;
    background: var(--raised);
    color: var(--ink);
    padding: 0;
    width: min(760px, calc(100vw - 24px));
    height: min(640px, calc(100dvh - 24px));
    max-height: none;
    box-shadow: var(--shadow);
    overflow: hidden;
  }
  dialog::backdrop {
    background: var(--scrim);
  }
  .box {
    height: 100%;
    display: grid;
    grid-template-columns: 172px minmax(0, 1fr);
    grid-template-rows: auto minmax(0, 1fr);
    grid-template-areas: 'head head' 'tabs panel';
  }
  header {
    grid-area: head;
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 14px 16px 12px 22px;
    border-bottom: 1px solid var(--rule);
  }
  .tabs {
    grid-area: tabs;
    display: grid;
    align-content: start;
    gap: 2px;
    padding: 12px 10px;
    border-right: 1px solid var(--rule);
    background: var(--paper-sunk);
  }
  .tabs button {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 36px;
    padding: 0 10px;
    border-radius: 8px;
    color: var(--ink-2);
    font-weight: 540;
    text-align: left;
  }
  .tabs button.on {
    background: var(--raised);
    color: var(--ink);
    box-shadow: inset 0 0 0 1px var(--rule);
  }
  .tabs button.on :global(svg) {
    color: var(--accent);
  }
  @media (hover: hover) {
    .tabs button:not(.on):hover {
      color: var(--ink);
      background: color-mix(in srgb, var(--raised) 60%, transparent);
    }
  }
  .panel {
    grid-area: panel;
    overflow-y: auto;
    padding: 20px 24px 26px;
    display: grid;
    align-content: start;
    grid-template-columns: minmax(0, 1fr);
    gap: 26px;
  }
  h2 {
    margin: 0;
    font-size: 20px;
    letter-spacing: -0.02em;
  }
  h3 {
    margin: 0 0 8px;
    font-size: 13.5px;
    color: var(--ink-2);
  }
  .seg {
    display: inline-flex;
    border: 1px solid var(--rule);
    border-radius: 10px;
    padding: 3px;
    gap: 2px;
    background: var(--paper);
  }
  .seg button {
    padding: 5px 16px;
    border-radius: 7px;
    font-weight: 540;
    color: var(--ink-2);
  }
  .seg button.on {
    background: var(--accent);
    color: var(--accent-ink);
  }
  .fx {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 8px;
  }
  .fx button {
    text-align: left;
    padding: 10px 12px;
    border: 1px solid var(--rule);
    border-radius: 10px;
    display: grid;
    gap: 2px;
  }
  @media (hover: hover) {
    .fx button:hover {
      border-color: var(--ink-3);
    }
  }

  .fx button.on {
    border-color: var(--accent);
    background: var(--accent-wash);
  }
  small,
  .rules { list-style: none; margin: 0 0 10px; padding: 0; display: grid; grid-template-columns: minmax(0, 1fr); gap: 4px; }
  .rules li { display: flex; align-items: center; gap: 8px; }
  .rules li .row { flex: 1; min-width: 0; }
  .rules li .row span { min-width: 0; overflow: hidden; }
  .rules li .btn { flex: none; }
  .rules small { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .note + .btn, .note + .newrule, .note + .rules { margin-top: 8px; }
  .newrule { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 2fr) auto; gap: 8px; }
  .note.err { color: var(--bad); }
  @media (max-width: 720px) { .newrule { grid-template-columns: minmax(0, 1fr); } }
  .note {
    color: var(--ink-3);
    font-size: 12.5px;
    margin: 0;
  }
  .sample {
    margin: 12px 0 0;
    padding: 12px 14px;
    border-radius: 10px;
    background: var(--paper);
    border: 1px solid var(--rule);
    min-height: 3em;
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
  .cur {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 4px 10px;
    margin: 0;
    font-size: 14px;
    overflow-wrap: anywhere;
  }
  .cur small {
    color: var(--ink-3);
    font-size: 12px;
  }
  .srv {
    display: flex;
    gap: 8px;
    margin-top: 8px;
  }
  .srv .btn {
    flex: none;
    white-space: nowrap;
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
      grid-template-columns: minmax(0, 1fr);
      grid-template-rows: auto auto minmax(0, 1fr);
      grid-template-areas: 'head' 'tabs' 'panel';
    }
    header {
      padding: calc(12px + env(safe-area-inset-top)) 12px 10px 16px;
    }
    .tabs {
      grid-template-columns: repeat(3, minmax(0, 1fr));
      padding: 8px 12px;
      border-right: 0;
      border-bottom: 1px solid var(--rule);
    }
    .tabs button {
      justify-content: center;
      gap: 6px;
      height: 34px;
      padding: 0 6px;
      font-size: 13.5px;
    }
    .panel {
      padding: 18px 16px calc(24px + env(safe-area-inset-bottom));
    }
  }
  @media (max-width: 520px) {
    .fx {
      grid-template-columns: 1fr;
    }
  }
  .subs {
    display: grid;
    gap: 2px;
    margin: 2px 0 0 26px;
    padding-left: 12px;
    border-left: 1px solid var(--rule);
  }
  .seg.off {
    opacity: 0.5;
  }
  .subs.off {
    opacity: 0.55;
  }
  .row.sub {
    align-items: flex-start;
  }
  .row.sub small {
    display: block;
    margin-top: 1px;
    font-size: 12px;
    line-height: 1.45;
    color: var(--ink-3);
  }
  .textrow {
    display: grid;
    gap: 6px;
    margin-top: 10px;
    font-size: 14px;
  }
  .textrow .field {
    width: 100%;
    box-sizing: border-box;
  }
</style>
