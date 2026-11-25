<script lang="ts">
  import { t } from '../lib/i18n.svelte';
  import { api } from '../lib/api';
  import { app } from '../lib/app.svelte';
  import type { Chat } from '../lib/chat.svelte';
  import type { ThinkingLevel } from '../lib/protocol';

  // Settings → Models: each model of the open session's list, whether the model menu shows it, and the reasoning level
  // it starts with (a new session on it, or switching to it). Unset, a model starts on its provider's recommended
  // level where the provider gives one (ChatGPT does), else on pi's; "pi" keeps pi's even then.
  let { chat }: { chat?: Chat } = $props();
  const LEVELS: ThinkingLevel[] = ['off', 'minimal', 'low', 'medium', 'high', 'xhigh', 'max'];

  let rec = $state<Record<string, Record<string, string>>>({});
  let failed = $state(false);
  $effect(() => {
    api.modelPrefs().then((p) => (app.modelPrefs = p)).catch(() => (failed = true));
    api.recommendedLevels().then((r) => (rec = r)).catch(() => {});
  });
  const prefs = $derived(app.modelPrefs);
  const models = $derived(chat?.models ?? []);

  async function change(provider: string, model: string, c: { level?: ThinkingLevel | 'pi' | null; shown?: boolean }) {
    try {
      app.modelPrefs = await api.updateModelPrefs(provider, model, c);
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
      {@const name = m.name || m.id}
      {@const recommended = rec[m.provider]?.[m.id]}
      {@const on = !prefs.hidden.includes(key)}
      <li class:off={!on}>
        <input class="switch" type="checkbox" role="switch" aria-label={t('settings.models.showFor', { model: name })} title={t('settings.models.showFor', { model: name })}
          checked={on} onchange={(e) => change(m.provider, m.id, { shown: e.currentTarget.checked })} />
        <span class="name"><b>{name}</b><small class="mono">{m.provider}</small></span>
        {#if m.reasoning !== false}
          <select class="field" aria-label={t('settings.models.levelFor', { model: name })} value={prefs.thinking[key] === 'pi' && !recommended ? '' : (prefs.thinking[key] ?? '')}
            onchange={(e) => change(m.provider, m.id, { level: (e.currentTarget.value || null) as ThinkingLevel | 'pi' | null })}>
            {#if recommended}
              <option value="">{t('settings.models.levelProvider', { level: recommended })}</option>
              <option value="pi">{t('settings.models.levelDefault')}</option>
            {:else}
              <option value="">{t('settings.models.levelDefault')}</option>
            {/if}
            {#each LEVELS as l (l)}<option value={l}>{l}</option>{/each}
          </select>
        {:else}
          <span class="nolevel">{t('settings.models.noReasoning')}</span>
        {/if}
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
  .switch {
    flex: none;
  }
  .name {
    flex: 1;
    min-width: 0;
    display: grid;
  }
  li.off .name {
    opacity: 0.5;
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
    width: 140px;
    flex: none;
  }
  .nolevel {
    width: 140px;
    flex: none;
    font-size: 12px;
    color: var(--ink-3);
    text-align: center;
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
