<script lang="ts">
  import { api, type OvenRelease } from '../lib/api';
  import { t } from '../lib/i18n.svelte';
  import { clock } from '../lib/util';
  import Markdown from './Markdown.svelte';
  import { app } from '../lib/app.svelte';

  // pi-oven updating itself: check the newest GitHub release by hash, then replace and restart with the same arguments.
  let { version }: { version: string } = $props();

  let build = $state<string | null>(null);
  let rel = $state<OvenRelease | null>(null);
  let phase = $state<'idle' | 'checking' | 'failed'>('idle');
  let error = $state('');

  $effect(() => { api.info().then((i) => (build = i.build ?? null)).catch(() => {}); });

  async function setAuto(on: boolean) {
    try { app.oven = await api.setOvenAuto(on); } catch (e) { error = (e as Error).message; }
  }

  async function check() {
    phase = 'checking';
    error = '';
    try { rel = await api.ovenCheck(); phase = 'idle'; void app.refreshUpdates(); }
    catch (e) { error = (e as Error).message; phase = 'failed'; }
  }

  const phaseNow = $derived(app.ovenPhase === 'idle' ? phase : app.ovenPhase);
  const apply = () => app.ovenUpdate(false);
</script>

<div class="oven">
  <p class="sub">pi-oven <span class="mono">{version}</span>{#if build}<span class="mono hash"> · {build}</span>{/if}</p>
  {#if phaseNow === 'restarting'}
    <p class="state">{t('oven.restarting')}</p>
  {:else if rel && !rel.upToDate}
    <p class="state new">{t('oven.available', { tag: rel.tag })} <span class="mono">{rel.current ?? '?'} → {rel.latest}</span></p>
    <button class="btn primary" type="button" disabled={phaseNow === 'applying'} onclick={apply}>{phaseNow === 'applying' ? t('oven.applying') : t('oven.apply')}</button>
    <p class="note">{t('oven.applyNote')}</p>
  {:else}
    {#if rel?.upToDate}<p class="state">{t('oven.upToDate', { tag: rel.tag })}</p>{/if}
    <button class="btn" type="button" disabled={phase === 'checking'} onclick={check}>{phase === 'checking' ? t('oven.checking') : t('oven.check')}</button>
  {/if}
  {#if rel?.notes && phaseNow !== 'restarting'}
    <details class="notes" open={!rel.upToDate}>
      <summary>{t('oven.notes')}{#if rel.updatedAt}<span class="when">{t('oven.builtAt', { date: new Date(rel.updatedAt).toLocaleDateString(), time: clock(Date.parse(rel.updatedAt)) })}</span>{/if}</summary>
      <div class="body"><Markdown source={rel.notes} /></div>
    </details>
  {/if}
  {#if error || app.ovenError}<p class="note err" role="alert">{error || app.ovenError}</p>{/if}
  {#if app.oven}
    <label class="auto">
      <span>{t('oven.auto')}<small>{t('oven.autoHint')}</small></span>
      <input class="switch" type="checkbox" role="switch" checked={app.oven.auto} onchange={(e) => setAuto(e.currentTarget.checked)} />
    </label>
  {/if}
</div>

<style>
  .oven {
    display: grid;
    justify-items: start;
    gap: 8px;
    margin: 4px 0 18px;
  }
  .sub {
    margin: 0;
    font-size: 12.5px;
    color: var(--ink-3);
  }
  .state {
    margin: 0;
    font-size: 13px;
    color: var(--ink-2);
  }
  .state.new {
    color: var(--accent);
    font-weight: 600;
  }
  .state .mono {
    font-weight: 400;
    color: var(--ink-3);
    margin-left: 6px;
  }
  .note {
    margin: 0;
    font-size: 12px;
    color: var(--ink-3);
  }
  .err {
    color: var(--bad);
  }
  .auto {
    width: 100%;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    font-size: 14px;
  }
  .auto small {
    display: block;
    color: var(--ink-3);
    font-size: 12px;
  }
  .notes {
    width: 100%;
    border: 1px solid var(--rule);
    border-radius: 10px;
  }
  .notes summary {
    padding: 8px 12px;
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
  }
  .when {
    margin-left: 8px;
    font-weight: 400;
    color: var(--ink-3);
    font-size: 12px;
  }
  .notes .body {
    max-height: 340px;
    overflow: auto;
    padding: 0 12px 10px;
    border-top: 1px solid var(--rule);
    font-size: 13px;
  }
</style>
