<script lang="ts">
  import { t } from '../lib/i18n.svelte';
  import { api, type ModelPrefs } from '../lib/api';
  import type { Chat } from '../lib/chat.svelte';
  import type { ThinkingLevel } from '../lib/protocol';

  // Settings → Models: the reasoning level each model starts with (a new session on it, or switching to it). The models
  // are the open session's list; models that do not reason have no level to set.
  let { chat }: { chat?: Chat } = $props();
  const LEVELS: ThinkingLevel[] = ['off', 'minimal', 'low', 'medium', 'high', 'xhigh', 'max'];

  let prefs = $state<ModelPrefs | null>(null);
  let failed = $state(false);
  $effect(() => { api.modelPrefs().then((p) => (prefs = p)).catch(() => (failed = true)); });

  const models = $derived((chat?.models ?? []).filter((m) => m.reasoning !== false));

  async function pick(provider: string, model: string, value: string) {
    try {
      prefs = await api.setModelLevel(provider, model, value ? (value as ThinkingLevel) : null);
    } catch {
      failed = true;
    }
  }
</script>

{#if !chat}
  <p class="note">{t('settings.models.levelsNoChat')}</p>
{:else if failed}
  <p class="note err">{t('settings.models.levelsFailed')}</p>
{:else if prefs}
  <ul>
    {#each models as m (`${m.provider}/${m.id}`)}
      {@const key = `${m.provider}/${m.id}`}
      <li>
        <span class="name"><b>{m.name || m.id}</b><small class="mono">{m.provider}</small></span>
        <select class="field" aria-label={t('settings.models.levelFor', { model: m.name || m.id })} value={prefs.thinking[key] ?? ''} onchange={(e) => pick(m.provider, m.id, e.currentTarget.value)}>
          <option value="">{t('settings.models.levelDefault')}</option>
          {#each LEVELS as l (l)}<option value={l}>{l}</option>{/each}
        </select>
      </li>
    {/each}
  </ul>
{/if}

<style>
  ul {
    list-style: none;
    margin: 8px 0 0;
    padding: 0;
    display: grid;
  }
  li {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 6px 0;
    border-top: 1px solid var(--rule);
  }
  li:first-child {
    border-top: 0;
  }
  .name {
    flex: 1;
    min-width: 0;
    display: grid;
  }
  .name b,
  .name small {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .name b {
    font-weight: 560;
    font-size: 14px;
  }
  .name small {
    font-size: 11px;
    color: var(--ink-3);
  }
  select {
    width: 128px;
    flex: none;
  }
  .note {
    margin: 6px 0 0;
    color: var(--ink-3);
    font-size: 13px;
  }
  .note.err {
    color: var(--bad);
  }
</style>
