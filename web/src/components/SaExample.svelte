<script lang="ts">
  // Settings → Theme: what the subagent choices look like, drawn by the real components from made-up runs and agents
  // (nothing is asked of pi or the server). `part`: the runs (chips or tray, whichever is chosen) or the info tab's
  // agents (tree or summary). Shown inert: it is a picture, the controls are the buttons above it.
  import { t } from '../lib/i18n.svelte';
  import { Subagents } from '../lib/subagents.svelte';
  import { settings } from '../lib/settings.svelte';
  import type { Chat } from '../lib/chat.svelte';
  import type { AgentDef } from '../lib/api';
  import type { BgTask } from '../lib/protocol';
  import type { Run } from '../addons/subagents';
  import BgChips from './BgChips.svelte';
  import SubagentTray from './SubagentTray.svelte';
  import SubagentInfo from './SubagentInfo.svelte';

  let { part }: { part: 'runs' | 'info' } = $props();

  const now = Date.now();
  const sa = new Subagents({ enabled: () => true, sessionFile: () => undefined, visible: () => false, addNotice: () => null });
  const runs: Run[] = [
    {
      toolId: 'ex-chain', shape: 'chain', background: true, state: 'running', startedAt: now - 252_000,
      steps: [
        { agent: 'scout', status: 'done', task: t('saExample.scout') },
        { agent: 'worker', status: 'running', task: t('saExample.worker'), tool: 'edit', toolArgs: 'src/limits.rs', turns: 6, tools: 14 },
        { agent: 'reviewer', status: 'pending', task: t('saExample.review') },
      ],
    },
    {
      toolId: 'ex-par', shape: 'parallel', background: true, state: 'running', startedAt: now - 95_000,
      steps: [
        { agent: 'reviewer', status: 'done', task: t('saExample.reviewA') },
        { agent: 'reviewer', status: 'running', task: t('saExample.reviewB'), tool: 'read', toolArgs: 'web/src/lib/chat.svelte.ts' },
        { agent: 'reviewer', status: 'running', task: t('saExample.reviewC'), tool: 'grep', toolArgs: 'queue_update' },
      ],
    },
  ];
  sa.runs = runs;
  const bg: BgTask[] = [{ id: 'ex-bg', name: 'cargo test', command: 'cargo test', status: 'running', startTime: now - 41_000 }];
  // only what the chips and the tray read
  const chat = { sa, bgChips: bg, bgFading: [], bgLanding: [], bgFocus: null, bgOpen: false, dismissBg() {} } as unknown as Chat;

  const agent = (name: string, kind: AgentDef['kind'], o: Partial<AgentDef> = {}): AgentDef => ({ name, kind, tools: 0, edits: false, ...o });
  const agents: AgentDef[] = [
    agent('scout', 'builtin', { model: 'claude-haiku-5-5', modelFrom: 'agent', tools: 4, description: t('saExample.scout') }),
    agent('reviewer', 'builtin', { model: 'claude-opus-5-5', modelFrom: 'override', thinking: 'high', tools: 5, description: t('saExample.review') }),
    agent('worker', 'builtin', { edits: true, description: t('saExample.worker') }),
    agent('planner', 'builtin', { edits: true, context: 'fork', model: 'claude-sonnet-5-5', modelFrom: 'default' }),
    agent('codex', 'external', { model: 'codex CLI', modelFrom: 'cli' }),
    agent('docs-writer', 'user', { edits: true, tools: 3 }),
  ];
</script>

<div class="ex" inert>
  <span class="tag">{t('saExample.tag')}</span>
  {#if part === 'runs'}
    <div class="stage">
      {#if settings.subagentView === 'tray'}<SubagentTray {chat} />{:else}<BgChips {chat} />{/if}
      <div class="box" aria-hidden="true">{t('saExample.box')}</div>
    </div>
  {:else}
    <SubagentInfo sample={{ agents, parent: 'Claude Opus 5.5', level: 'medium' }} />
  {/if}
</div>

<style>
  .ex {
    position: relative;
    margin-top: 10px;
    padding: 26px 12px 12px;
    border: 1px dashed var(--rule);
    border-radius: 10px;
    background: var(--paper-sunk);
    overflow: hidden;
  }
  .tag {
    position: absolute;
    top: 7px;
    left: 12px;
    font: 600 10.5px var(--font-mono);
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--ink-3);
  }
  /* a plain block, as over the real message box: the tray centres itself with auto margins, which in a flex column
     would size it to its content and push it past the edge */
  .stage {
    display: block;
    min-width: 0;
  }
  /* where the message box would be, so the runs sit as they do over it */
  .box {
    margin-top: 8px;
    padding: 11px 14px;
    border: 1px solid var(--rule);
    border-radius: 18px;
    background: var(--paper);
    color: var(--ink-3);
    font-size: 13px;
  }
</style>
