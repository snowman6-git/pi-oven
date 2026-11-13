<script lang="ts">
  import { api, type About } from '../lib/api';
  import { app } from '../lib/app.svelte';
  import { t } from '../lib/i18n.svelte';
  import OvenUpdate from './OvenUpdate.svelte';

  // Settings → About: pi first and large, then what it runs with.
  let info = $state<About | null>(null);
  let failed = $state(false);
  $effect(() => {
    api.about().then((a) => (info = a), () => (failed = true));
  });

  const offer = $derived(app.updates.offer);
</script>

<section class="about">
  {#if failed}
    <p class="note">{t('about.failed')}</p>
  {:else if !info}
    <p class="note">{t('about.loading')}</p>
  {:else}
    <div class="hero">
      <span class="lbl">pi</span>
      <span class="ver mono">{info.pi ?? '—'}</span>
      {#if offer?.pi}<span class="new">{t('about.newVersion', { v: offer.pi[1] })}</span>{/if}
    </div>
    <OvenUpdate version={info.oven} />

    <h3>{t('about.packages')} <span class="n">{info.packages.length}</span></h3>
    {#if info.packages.length}
      <ul>
        {#each info.packages as p (p.source + p.name)}
          <li>
            <span class="name mono" data-priv={p.source.startsWith('npm:') ? undefined : 'paths'}>{p.name}</span>
            {#if offer?.extensions.includes(p.name)}<span class="new">{t('about.updatable')}</span>{/if}
            <span class="v mono">{p.version ?? (p.source.startsWith('npm:') ? '' : p.source)}</span>
            {#if p.description}<small>{p.description}</small>{/if}
          </li>
        {/each}
      </ul>
    {:else}
      <p class="note">{t('about.noPackages')}</p>
    {/if}

    <h3>{t('about.mcp')} <span class="n">{info.mcp.length}</span></h3>
    {#if info.mcp.length}
      <ul>
        {#each info.mcp as m (m.name)}
          <li class:off={m.disabled}>
            <span class="name mono">{m.name}</span>
            <span class="v">
              {[m.transport, m.tools != null ? t('about.tools', { n: m.tools }) : null, m.prompts ? t('about.prompts', { n: m.prompts }) : null,
                m.disabled ? t('about.disabled') : null, !m.configured ? t('about.projectOnly') : null].filter(Boolean).join(' · ')}
            </span>
          </li>
        {/each}
      </ul>
      <p class="note">{t('about.mcpNote')}</p>
    {:else}
      <p class="note">{t('about.noMcp')}</p>
    {/if}
  {/if}
</section>

<style>
  .hero {
    display: flex;
    align-items: baseline;
    flex-wrap: wrap;
    gap: 6px 12px;
  }
  .lbl {
    font-size: 15px;
    font-weight: 650;
    color: var(--ink-2);
  }
  .ver {
    font-size: 30px;
    font-weight: 600;
    letter-spacing: -0.02em;
    line-height: 1.1;
  }
  h3 {
    margin: 16px 0 6px;
    font-size: 13px;
  }
  .n {
    color: var(--ink-3);
    font-weight: 500;
    margin-left: 4px;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    border: 1px solid var(--rule);
    border-radius: 8px;
  }
  li {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 2px 8px;
    padding: 7px 10px;
    font-size: 12.5px;
  }
  li + li {
    border-top: 1px solid var(--rule);
  }
  li.off {
    opacity: 0.55;
  }
  .name {
    min-width: 0;
    overflow-wrap: anywhere;
    color: var(--ink);
  }
  .v {
    margin-left: auto;
    color: var(--ink-3);
    font-size: 12px;
  }
  small {
    flex-basis: 100%;
    color: var(--ink-3);
    font-size: 11.5px;
    line-height: 1.4;
  }
  .new {
    font-size: 11px;
    padding: 1px 7px;
    border-radius: 999px;
    background: var(--accent-wash);
    color: var(--accent);
    font-weight: 600;
  }
  .note {
    margin: 6px 0 0;
    font-size: 12px;
    color: var(--ink-3);
  }
</style>
