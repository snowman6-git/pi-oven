<script lang="ts">
  import { api, type OvenRelease } from '../lib/api';
  import { t } from '../lib/i18n.svelte';

  // pi-oven updating itself: check the newest GitHub release by hash, then replace and restart with the same arguments.
  let { version }: { version: string } = $props();

  let build = $state<string | null>(null);
  let rel = $state<OvenRelease | null>(null);
  let phase = $state<'idle' | 'checking' | 'applying' | 'restarting' | 'failed'>('idle');
  let error = $state('');

  $effect(() => { api.info().then((i) => (build = i.build ?? null)).catch(() => {}); });

  async function check() {
    phase = 'checking';
    error = '';
    try { rel = await api.ovenCheck(); phase = 'idle'; }
    catch (e) { error = (e as Error).message; phase = 'failed'; }
  }

  async function apply() {
    if (!rel) return;
    phase = 'applying';
    error = '';
    const want = rel.latest;
    try { await api.ovenApply(); }
    catch (e) { error = (e as Error).message; phase = 'failed'; return; }
    phase = 'restarting';
    // the server goes away and comes back as the new build: wait for it, then load its page
    // a restart takes well under a second, so do not wait to see it go down: a different build answering is enough
    // (an older release does not say its build at all, which also differs)
    const before = build;
    const end = Date.now() + 90_000;
    while (Date.now() < end) {
      await new Promise((r) => setTimeout(r, 700));
      const now = await api.info().catch(() => null);
      if (now && (now.build === want || (now.build ?? null) !== before)) { location.reload(); return; }
    }
    error = t('oven.notBack');
    phase = 'failed';
  }
</script>

<div class="oven">
  <p class="sub">pi-oven <span class="mono">{version}</span>{#if build}<span class="mono hash"> · {build}</span>{/if}</p>
  {#if phase === 'restarting'}
    <p class="state">{t('oven.restarting')}</p>
  {:else if rel && !rel.upToDate}
    <p class="state new">{t('oven.available', { tag: rel.tag })} <span class="mono">{rel.current ?? '?'} → {rel.latest}</span></p>
    <button class="btn primary" type="button" disabled={phase === 'applying'} onclick={apply}>{phase === 'applying' ? t('oven.applying') : t('oven.apply')}</button>
    <p class="note">{t('oven.applyNote')}</p>
  {:else}
    {#if rel?.upToDate}<p class="state">{t('oven.upToDate', { tag: rel.tag })}</p>{/if}
    <button class="btn" type="button" disabled={phase === 'checking'} onclick={check}>{phase === 'checking' ? t('oven.checking') : t('oven.check')}</button>
  {/if}
  {#if error}<p class="note err" role="alert">{error}</p>{/if}
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
</style>
