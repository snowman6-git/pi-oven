<script lang="ts">
  import { S } from '../lib/strings';
  import { t } from '../lib/i18n.svelte';
  import { onMount, tick, untrack } from 'svelte';
  import { app } from '../lib/app.svelte';
  import { showcase } from '../lib/showcase.svelte';
  import { pinRules } from '../lib/pinrules.svelte';
  import type { Chat } from '../lib/chat.svelte';
  import { PointScanner, displayUrl, isLocalHost, mask, reachable, type SecretPoint, type UrlPoint } from '../lib/points';
  import type { Pin } from '../lib/pins.svelte';
  import Icon from './Icon.svelte';

  let { chat }: { chat: Chat } = $props();

  const pins = $derived(chat.pins.list);
  let found = $state<{ urls: UrlPoint[]; secrets: SecretPoint[] }>({ urls: [], secrets: [] });
  let shown = $state<Record<string, boolean>>({});
  let opened = $state<Record<string, boolean>>({});
  let moreUrls = $state(false);
  let label = $state('');
  let value = $state('');

  const scanner = new PointScanner();
  const rescan = () => (found = scanner.scan(chat.items));
  // Changed rules: read everything again.
  $effect(() => {
    const rules = pinRules.compiled;
    untrack(() => { scanner.setRules(rules); rescan(); });
  });

  onMount(() => {
    rescan();
    // Cheap enough to repeat; and only changed items are read again.
    const t = setInterval(rescan, 1500);
    return () => clearInterval(t);
  });

  // A session that just loaded, or a message that just arrived, should show up without waiting for the next tick.
  $effect(() => {
    chat.items.length;
    chat.loaded;
    untrack(rescan);
  });

  const pinned = (v: string) => chat.pins.hasValue(v);
  const pin = (kind: 'url' | 'secret' | 'note', lbl: string, v: string) => chat.pins.add(kind, lbl, v);
  const unpin = (p: Pin) => chat.pins.remove(p.id);

  /** Back to the message in the conversation (if it is still there), and a moment of light on it. */
  async function jump(p: Pin) {
    chat.editors.activate(null);
    await tick();
    const el = document.querySelector<HTMLElement>(`[data-msg="${CSS.escape(p.mkey ?? '')}"]`);
    if (!el) { app.flash(t('point.notFound')); return; }
    if (app.narrow) app.inspectorDrawer = false;
    el.scrollIntoView({ block: 'center', behavior: 'smooth' });
    el.classList.add('pinflash');
    setTimeout(() => el.classList.remove('pinflash'), 1800);
  }
  const long = (t: string) => t.split('\n').length > 3 || t.length > 180;

  function addByHand() {
    const v = value.trim();
    if (!v) return;
    const isUrl = /^https?:\/\//i.test(v);
    pin(isUrl ? 'url' : 'note', label.trim() || (isUrl ? new URL(v).host : t('point.memo')), v);
    label = value = '';
  }

  const host = typeof location === 'undefined' ? '' : location.hostname;
  function open(u: string) {
    window.open(reachable(u, host), '_blank', 'noopener,noreferrer');
  }
  const swapped = (u: string) => (reachable(u, host) !== u ? reachable(u, host) : null);
  const urlsShown = $derived(moreUrls ? found.urls : found.urls.slice(0, 6));
</script>

<div class="tab">
  <section>
    <h4>{t('point.pinnedTitle')}</h4>
    {#if pins.length === 0}
      <p class="note">{t('point.pinnedNote')}</p>
    {/if}
    <ul>
      {#each pins as p (p.id)}
        {#if p.kind === 'message'}
        <li class="pin msg">
          <div class="top">
            <span class="who" class:me={p.role === 'user'}>{p.role === 'user' ? t('point.me') : 'pi'}</span>
            <span class="lbl" data-priv="titles">{p.label}</span>
            <span class="btns">
              <button class="icon-btn sm" type="button" aria-label={t('point.inChat')} title={t('point.inChat')} onclick={() => jump(p)}><Icon name="chat" size={15} /></button>
              <button class="icon-btn sm" type="button" aria-label={S.copy} title={S.copy} onclick={() => app.copy(p.value)}><Icon name="copy" size={15} /></button>
              <button class="icon-btn sm" type="button" aria-label={t('point.unpin')} title={t('point.unpin')} onclick={() => unpin(p)}><Icon name="x" size={15} /></button>
            </span>
          </div>
          <p class="mtext" data-priv="chat" class:open={opened[p.id]}>{showcase.text(p.value)}</p>
          {#if long(p.value)}<button class="more" type="button" aria-expanded={!!opened[p.id]} onclick={() => (opened[p.id] = !opened[p.id])}>{opened[p.id] ? S.showLess : t('point.expand')}</button>{/if}
        </li>
        {:else}
        <li class="pin">
          <div class="top"><span class="lbl">{p.label}</span>
            <span class="btns">
              {#if p.kind === 'secret'}<button class="icon-btn sm" type="button" aria-label={shown[p.id] ? t('point.hide') : t('point.show')} onclick={() => (shown[p.id] = !shown[p.id])}><Icon name={shown[p.id] ? 'eyeOff' : 'eye'} size={15} /></button>{/if}
              <button class="icon-btn sm" type="button" aria-label={S.copy} title={S.copy} onclick={() => app.copy(p.value)}><Icon name="copy" size={15} /></button>
              {#if p.kind === 'url'}<button class="icon-btn sm" type="button" aria-label={t('point.open')} title={swapped(p.value) ? t('point.openAs', { url: swapped(p.value) ?? '' }) : t('point.open')} onclick={() => open(p.value)}><Icon name="external" size={15} /></button>{/if}
              <button class="icon-btn sm" type="button" aria-label={t('point.unpin')} title={t('point.unpin')} onclick={() => unpin(p)}><Icon name="x" size={15} /></button>
            </span>
          </div>
          {#if p.kind === 'url'}
            <a class="mono val link" data-priv="hosts" href={reachable(p.value, host)} target="_blank" rel="noopener noreferrer">{displayUrl(p.value)}</a>
          {:else}
            <code class="mono val" data-priv={p.kind === 'secret' ? 'secrets' : undefined}>{p.kind === 'secret' ? (shown[p.id] ? p.value : mask(p.value)) : p.value}</code>
          {/if}
        </li>
        {/if}
      {/each}
    </ul>
    <form class="add" onsubmit={(e) => { e.preventDefault(); addByHand(); }}>
      <input class="field" placeholder={t('point.name')} aria-label={t('point.name')} bind:value={label} />
      <input class="field" placeholder={t('point.valuePh')} aria-label={t('point.value')} bind:value />
      <button class="btn" type="submit" disabled={!value.trim()}>{t('point.pin')}</button>
    </form>
  </section>

  <section>
    <h4>{t('point.urls')} <span class="cnt">{found.urls.length}</span></h4>
    {#if found.urls.length === 0}<p class="note">{t('point.noUrls')}</p>{/if}
    <ul>
      {#each urlsShown as u (u.value)}
        <li class="hit">
          <div class="top">
            <a class="mono val link" data-priv="hosts" href={reachable(u.value, host)} target="_blank" rel="noopener noreferrer">{displayUrl(u.value)}</a>
            <span class="btns">
              <button class="icon-btn sm" type="button" aria-label={S.copy} title={S.copy} onclick={() => app.copy(u.value)}><Icon name="copy" size={15} /></button>
              <button class="icon-btn sm" type="button" aria-label={t('point.open')} title={swapped(u.value) ? t('point.openAs', { url: swapped(u.value) ?? '' }) : t('point.open')} onclick={() => open(u.value)}><Icon name="external" size={15} /></button>
              <button class="icon-btn sm" type="button" aria-label={t('point.pin')} title={t('point.pin')} disabled={pinned(u.value)} onclick={() => pin('url', u.host + (u.port ? ':' + u.port : ''), u.value)}><Icon name="pin" size={15} /></button>
            </span>
          </div>
          <small>{#if isLocalHost(u.host) && swapped(u.value)}{t('point.local', { host })}{:else}{t('point.seen', { n: u.count })}{/if}</small>
        </li>
      {/each}
    </ul>
    {#if found.urls.length > 6}
      <button class="more" type="button" onclick={() => (moreUrls = !moreUrls)}>{moreUrls ? S.showLess : `${S.showMore} (${found.urls.length - 6})`}</button>
    {/if}
  </section>

  <section>
    <h4>{t('point.secrets')} <span class="cnt">{found.secrets.length}</span></h4>
    {#if found.secrets.length === 0}<p class="note">{t('point.noSecrets')}</p>{/if}
    <ul>
      {#each found.secrets as s (s.value)}
        <li class="hit">
          <div class="top">
            <span class="lbl">{s.label}</span>
            <span class="btns">
              <button class="icon-btn sm" type="button" aria-label={shown[s.value] ? t('point.hide') : t('point.show')} onclick={() => (shown[s.value] = !shown[s.value])}><Icon name={shown[s.value] ? 'eyeOff' : 'eye'} size={15} /></button>
              <button class="icon-btn sm" type="button" aria-label={S.copy} title={S.copy} onclick={() => app.copy(s.value)}><Icon name="copy" size={15} /></button>
              <button class="icon-btn sm" type="button" aria-label={t('point.pin')} title={t('point.pin')} disabled={pinned(s.value)} onclick={() => pin('secret', s.label, s.value)}><Icon name="pin" size={15} /></button>
            </span>
          </div>
          <code class="mono val" data-priv="secrets">{shown[s.value] ? s.value : mask(s.value)}</code>
        </li>
      {/each}
    </ul>
    <button class="btn quiet rules" type="button" onclick={() => app.openSettings('pins')}>{t('point.rules', { n: pinRules.list.length })}</button>
  </section>
</div>

<style>
  .tab { display: grid; gap: 18px; min-width: 0; }
  section { display: grid; gap: 8px; min-width: 0; }
  h4 { margin: 0; font-size: 13px; font-weight: 700; color: var(--ink-2); }
  .cnt { color: var(--ink-3); font-weight: 500; margin-left: 2px; }
  .rules { justify-self: start; margin-top: 4px; font-size: 12.5px; }
  .note { margin: 0; color: var(--ink-3); font-size: 12.5px; }
  small { color: var(--ink-3); font-size: 11.5px; }
  ul { list-style: none; margin: 0; padding: 0; display: grid; gap: 6px; }
  li { border: 1px solid var(--rule); border-radius: 8px; background: var(--paper); padding: 7px 8px 7px 10px; display: grid; gap: 2px; min-width: 0; }
  li.pin { border-color: color-mix(in srgb, var(--heat) 45%, var(--rule)); }
  .top { display: flex; align-items: center; justify-content: space-between; gap: 6px; min-width: 0; }
  .lbl { font-size: 12.5px; font-weight: 650; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .val { font-size: 12px; color: var(--ink-2); overflow-wrap: anywhere; min-width: 0; }
  .hit .top .val { flex: 1; }
  .link { color: var(--accent); text-decoration: underline; text-decoration-color: color-mix(in srgb, var(--accent) 40%, transparent); text-underline-offset: 2px; }
  .link:hover { text-decoration-color: currentColor; }
  .btns { display: flex; flex: none; }
  .sm { width: 26px; height: 26px; }
  .pin.msg { gap: 4px; }
  .pin.msg .top { align-items: center; }
  .who { flex: none; padding: 0 7px; border-radius: 999px; background: var(--paper-sunk); color: var(--ink-2); font-size: 11.5px; font-weight: 650; }
  .who.me { background: var(--accent-wash); color: var(--accent); }
  .pin.msg .lbl { flex: 1; font-weight: 560; }
  .mtext { margin: 0; font-size: 12.5px; line-height: 1.55; color: var(--ink-2); white-space: pre-wrap; overflow-wrap: anywhere; display: -webkit-box; -webkit-box-orient: vertical; -webkit-line-clamp: 3; line-clamp: 3; overflow: hidden; }
  .mtext.open { display: block; max-height: 40dvh; overflow: auto; }
  .add { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 2fr) auto; gap: 6px; }
  .add .field { height: 32px; font-size: 13px; min-width: 0; }
  .more { justify-self: start; font-size: 12.5px; color: var(--ink-3); }
</style>
