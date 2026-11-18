<script lang="ts">
  // The info tab, under the plan cards: the subagents set up in pi-subagents (only when it is installed). Settings →
  // Theme picks the form: a family tree (this session at the root; read-only, writing, external-CLI and own agents as
  // branches) or one line that opens. An agent with no model of its own runs on this session's model: that is marked
  // (in the accent), since it spends the same plan. Any agent opens its details. Definitions are read again when the
  // page comes back into view, so an edited agent file shows without a reload.
  import { t } from '../lib/i18n.svelte';
  import { api, type AgentDef } from '../lib/api';
  import { settings } from '../lib/settings.svelte';
  import { showcase } from '../lib/showcase.svelte';
  import type { Chat } from '../lib/chat.svelte';

  // `sample`: made-up agents to draw instead (Settings → Theme's example), with no session behind them
  let { chat, sample }: { chat?: Chat; sample?: { agents: AgentDef[]; parent: string; level: string } } = $props();

  let fetched = $state<AgentDef[] | null>(null);
  const agents = $derived(sample?.agents ?? fetched);
  const shown = $derived(!!sample || !!chat?.hasSubagents);
  // a failed read (a bad link) keeps what was there rather than emptying the section
  const read = (id: string) => api.subagentAgents(id).then((r) => (fetched = r.agents)).catch(() => {});
  // asked when the tab shows, again for another session (another folder may have project agents), and when the page
  // comes back into view (the definitions are files the person edits elsewhere)
  $effect(() => {
    if (!chat?.hasSubagents) return;
    const id = chat.runtime.id;
    fetched = null; // another session: its own folder's agents
    void read(id);
    const again = () => document.visibilityState === 'visible' && void read(id);
    window.addEventListener('focus', again);
    document.addEventListener('visibilitychange', again);
    return () => {
      window.removeEventListener('focus', again);
      document.removeEventListener('visibilitychange', again);
    };
  });

  type Fam = 'ro' | 'wr' | 'ex' | 'own';
  const famOf = (a: AgentDef): Fam => (a.kind === 'external' ? 'ex' : a.kind === 'user' || a.kind === 'project' ? 'own' : a.edits ? 'wr' : 'ro');
  const FAMS: Fam[] = ['ro', 'wr', 'ex', 'own'];
  const byFam = $derived(FAMS.map((f) => ({ f, list: (agents ?? []).filter((a) => famOf(a) === f) })).filter((g) => g.list.length));
  const inherit = $derived((agents ?? []).filter((a) => !a.model).length);
  const writers = $derived((agents ?? []).filter((a) => a.edits).length);
  const parent = $derived(sample?.parent ?? chat?.state?.model?.name ?? chat?.state?.model?.id ?? '');
  const level = $derived(sample?.level ?? chat?.state?.thinkingLevel ?? '');

  let open = $state(false);
  let sel = $state<string | null>(null);
  const picked = $derived(agents?.find((a) => a.name === sel) ?? null);
  const toggle = (name: string) => (sel = sel === name ? null : name);
  // one's own agents can be named after private work
  const priv = (a: AgentDef) => (a.kind === 'user' || a.kind === 'project' ? 'titles' : undefined);
  const tipOf = (a: AgentDef) => (priv(a) ? showcase.tip('titles', a.description ?? undefined) : (a.description ?? undefined));
</script>

{#snippet detail(a: AgentDef)}
  <div class="detail" role="region" aria-label={a.name}>
    <div class="dh"><i class="dot {famOf(a)}"></i><b data-priv={priv(a)}>{showcase.text(a.name)}</b><small>{t(`saInfo.kind.${a.kind}`)}</small>
      <button class="x" type="button" aria-label={t('saInfo.close')} onclick={() => (sel = null)}>×</button>
    </div>
    {#if a.description}<p data-priv={priv(a)}>{showcase.text(a.description)}</p>{/if}
    <dl>
      <dt>{t('saInfo.model')}</dt>
      <dd>{#if a.model}<span class="mono" data-priv="hosts">{showcase.text(a.model)}</span>{#if a.modelFrom}<small> · {t(`saInfo.from.${a.modelFrom}`)}</small>{/if}{:else}<span class="inh">{t('saInfo.inheritsLong')}</span>{/if}</dd>
      {#if a.thinking}<dt>{t('saInfo.thinking')}</dt><dd class="mono">{a.thinking}</dd>{/if}
      <dt>{t('saInfo.toolsLabel')}</dt>
      <dd>{a.tools ? t('saInfo.tools', { n: a.tools }) : t('saInfo.allTools')} · {a.edits ? t('saInfo.writes') : t('saInfo.readOnly')}</dd>
      {#if a.context === 'fork'}<dt>{t('saInfo.fork')}</dt><dd></dd>{/if}
    </dl>
  </div>
{/snippet}

{#if shown && agents?.length}
  <section class="sa">
    {#if settings.subagentInfo === 'tree'}
      <h3>{t('saInfo.tree')}<span class="n">{agents.length}</span></h3>
      <div class="tree">
        <div class="root"><b>{t('saInfo.root')}</b>{#if parent}<small>{parent}{level ? ` · ${level}` : ''}</small>{/if}</div>
        <div class="branch">
          {#each byFam as g (g.f)}
            <div class="fam">
              <div class="fh"><i class="dot {g.f}"></i>{t(`saInfo.fam.${g.f}`)}<small>{g.list.length}</small></div>
              <ul class="kids">
                {#each g.list as a (a.name)}
                  <li title={tipOf(a)}>
                    <button class="nm" type="button" aria-expanded={sel === a.name} onclick={() => toggle(a.name)} data-priv={priv(a)}>{showcase.text(a.name)}</button>
                    {#if a.model}<span class="own" data-priv="hosts">{showcase.text(a.model)}</span>{:else}<span class="inh">{t('saInfo.inherits')}</span>{/if}
                    {#if a.context === 'fork'}<span class="own">· {t('saInfo.fork')}</span>{/if}
                  </li>
                {/each}
              </ul>
            </div>
          {/each}
        </div>
      </div>
      {#if picked}{@render detail(picked)}{/if}
    {:else}
      <h3>{t('saInfo.title')}</h3>
      <button class="sum" type="button" aria-expanded={open} onclick={() => (open = !open)}>
        <span class="segs" aria-hidden="true">{#each byFam as g (g.f)}{#each g.list as a (a.name)}<i class="dot {g.f}"></i>{/each}{/each}</span>
        <span class="line"><b>{t('saInfo.count', { n: agents.length })}</b>{#if inherit} · <span class="inh">{t('saInfo.inheritN', { n: inherit })}</span>{/if}{#if writers} · {t('saInfo.writersN', { n: writers })}{/if}</span>
      </button>
      {#if open}
        <div class="chips">
          {#each agents as a (a.name)}
            <button class="chip" class:on={sel === a.name} type="button" aria-expanded={sel === a.name} title={tipOf(a)} onclick={() => toggle(a.name)}><i class="dot {famOf(a)}"></i><span data-priv={priv(a)}>{showcase.text(a.name)}</span>{#if !a.model}<span class="inh">↳</span>{/if}</button>
          {/each}
        </div>
        {#if picked}{@render detail(picked)}{/if}
      {/if}
    {/if}
    <div class="legend" aria-hidden="true">{#each FAMS as f (f)}<span><i class="dot {f}"></i>{t(`saInfo.fam.${f}`)}</span>{/each}</div>
  </section>
{/if}

<style>
  h3 {
    margin: 0 0 8px;
    font-size: 13px;
    font-weight: 700;
    color: var(--ink-2);
    display: flex;
    align-items: center;
    gap: 6px;
  }
  h3 .n {
    margin-left: auto;
    font: 500 12px var(--font-mono);
    color: var(--ink-3);
  }
  small {
    color: var(--ink-3);
    font-size: 12px;
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 2px;
    flex: none;
    display: inline-block;
    background: var(--ink-3);
  }
  .dot.ro {
    background: var(--ok);
  }
  .dot.wr {
    background: var(--accent);
  }
  .dot.ex {
    background: var(--ft-blue);
  }
  .dot.own {
    background: var(--ft-violet);
  }
  /* runs on this session's model: the accent, not the amber (that is for work under way and running low) */
  .inh {
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--accent);
  }
  .own {
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--ink-3);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
  }
  /* the family tree */
  .tree {
    display: flex;
    flex-direction: column;
    font-size: 12.5px;
  }
  .root {
    align-self: flex-start;
    display: flex;
    align-items: baseline;
    gap: 8px;
    padding: 5px 10px;
    border: 1px solid var(--accent);
    border-radius: 8px;
    background: var(--accent-wash);
  }
  .root small {
    font-family: var(--font-mono);
    font-size: 11px;
  }
  .branch {
    margin-left: 16px;
    padding-left: 14px;
    border-left: 1px solid var(--rule);
    display: flex;
    flex-direction: column;
  }
  .fam {
    position: relative;
    padding-top: 10px;
  }
  .fam::before {
    content: '';
    position: absolute;
    left: -14px;
    top: 19px;
    width: 10px;
    border-top: 1px solid var(--rule);
  }
  .fh {
    display: flex;
    align-items: center;
    gap: 7px;
    color: var(--ink-2);
  }
  .kids {
    list-style: none;
    margin: 6px 0 0 3px;
    padding: 0 0 0 14px;
    border-left: 1px dashed var(--rule);
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .kids li {
    position: relative;
    display: flex;
    align-items: baseline;
    gap: 6px;
    min-width: 0;
  }
  .kids li::before {
    content: '';
    position: absolute;
    left: -14px;
    top: 0.75em;
    width: 10px;
    border-top: 1px dashed var(--rule);
  }
  .nm {
    font: inherit;
    font-weight: 560;
    flex: none;
    padding: 0;
    border: 0;
    background: none;
    color: inherit;
    cursor: pointer;
    text-align: left;
  }
  .nm[aria-expanded='true'] {
    color: var(--accent);
  }
  @media (hover: hover) {
    .nm:hover {
      text-decoration: underline;
      text-underline-offset: 3px;
    }
    .chip:hover {
      background: var(--paper-sunk);
    }
  }
  .mono {
    font-family: var(--font-mono);
    font-size: 11.5px;
  }
  /* one agent, opened */
  .detail {
    margin-top: 10px;
    padding: 10px 12px;
    border: 1px solid var(--rule);
    border-radius: 10px;
    font-size: 12.5px;
    color: var(--ink-2);
  }
  .dh {
    display: flex;
    align-items: center;
    gap: 7px;
  }
  .dh b {
    color: var(--ink);
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .x {
    margin-left: auto;
    width: 24px;
    height: 24px;
    border: 0;
    border-radius: 6px;
    background: none;
    color: var(--ink-3);
    font-size: 16px;
    line-height: 1;
    cursor: pointer;
  }
  .detail p {
    margin: 6px 0 0;
    overflow-wrap: anywhere;
  }
  .detail dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 4px 12px;
    margin: 8px 0 0;
  }
  .detail dt {
    color: var(--ink-3);
  }
  .detail dd {
    margin: 0;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  /* the folded line */
  .sum {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 9px 12px;
    border: 1px solid var(--rule);
    border-radius: 10px;
    background: var(--paper);
    font-size: 12.5px;
    color: var(--ink-2);
    text-align: left;
  }
  @media (hover: hover) {
    .sum:hover {
      background: var(--paper-sunk);
    }
  }
  .segs {
    display: inline-flex;
    gap: 3px;
    flex: none;
  }
  .segs .dot {
    width: 5px;
    height: 14px;
  }
  .line b {
    color: var(--ink);
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 5px;
    margin-top: 8px;
  }
  .chip {
    font: inherit;
    font-size: 12px;
    background: none;
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 2px 8px;
    border: 1px solid var(--rule);
    border-radius: 8px;
    color: var(--ink-2);
  }
  .chip.on {
    border-color: var(--accent);
  }
  .legend {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
    margin-top: 10px;
    font-size: 11px;
    color: var(--ink-3);
  }
  .legend span {
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }
</style>
