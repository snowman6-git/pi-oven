<script lang="ts">
  import { app } from '../lib/app.svelte';
  import type { Chat, Permission } from '../lib/chat.svelte';
  import { dismiss, autofocus } from '../lib/actions';
  import type { ModelInfo, ThinkingLevel } from '../lib/protocol';
  import { S } from '../lib/strings';
  import { compactNumber } from '../lib/util';
  import ContextRing from './ContextRing.svelte';
  import Icon from './Icon.svelte';

  type Menu = 'plus' | 'perm' | 'model' | 'think' | 'ctx' | null;

  // The row under the message box: permission, model, thinking level, context. `menu` is shared with the attach menu
  // in Composer so that only one is open at a time.
  let { chat, menu = $bindable(), toggle }: { chat: Chat; menu: Menu; toggle: (m: Exclude<Menu, null>) => void } = $props();

  let filter = $state('');
  $effect(() => {
    menu;
    filter = '';
  });

  const models = $derived(
    chat.models.filter((m) => `${m.provider} ${m.id} ${m.name ?? ''}`.toLowerCase().includes(filter.toLowerCase())),
  );
  const model = $derived(chat.state?.model);
  const sameModel = (m: ModelInfo) => m.id === model?.id && m.provider === model?.provider;
  const ctx = $derived(chat.stats?.contextUsage);
  const pct = $derived(chat.contextPercent);

  const perms: { id: Permission; label: string; hint: string }[] = [
    { id: 'auto', label: '자동', hint: '도구를 묻지 않고 바로 실행합니다.' },
    { id: 'manual', label: '수동', hint: '명령 실행과 파일 수정 전에 허락을 구합니다.' },
  ];
  const permLabel = $derived(perms.find((p) => p.id === chat.permission)!.label);
</script>

<div class="meta">
  <div class="pop" use:dismiss={() => { if (menu === 'perm') menu = null; }}>
    {#if chat.hasPermissionExt}
      <button class="chip" class:manual={chat.permission === 'manual'} type="button" aria-haspopup="menu" aria-expanded={menu === 'perm'} onclick={() => toggle('perm')}>
        <Icon name="shield" size={14} />{permLabel}<Icon name="down" size={12} />
      </button>
      {#if menu === 'perm'}
        <div class="sheet" role="menu">
          {#each perms as p (p.id)}
            <button role="menuitemradio" aria-checked={chat.permission === p.id} type="button" class:on={chat.permission === p.id}
              onclick={() => ((menu = null), chat.setPermission(p.id))}>
              <span class="optlabel"><b>{p.label}</b><small>{p.hint}</small></span>
              {#if chat.permission === p.id}<Icon name="check" size={16} />{/if}
            </button>
          {/each}
        </div>
      {/if}
    {/if}
  </div>

  <span class="grow"></span>

  <div class="pop" use:dismiss={() => { if (menu === 'model') menu = null; }}>
    <button class="chip" type="button" aria-haspopup="listbox" aria-expanded={menu === 'model'} onclick={() => toggle('model')}>
      <span class="mname">{model?.name ?? model?.id ?? S.model}</span><Icon name="down" size={12} />
    </button>
    {#if menu === 'model'}
      <div class="sheet wide">
        {#if chat.models.length > 7}
          <input class="field" placeholder="모델 검색" bind:value={filter} use:autofocus />
        {/if}
        <ul role="listbox" aria-label={S.model}>
          {#each models as m (m.provider + m.id)}
            <li role="option" aria-selected={sameModel(m)}>
              <button type="button" class:on={sameModel(m)} onclick={() => ((menu = null), chat.setModel(m))}>
                <span>{m.name ?? m.id}</span><small>{m.provider}</small>
              </button>
            </li>
          {:else}
            <li class="none">일치하는 모델이 없습니다.</li>
          {/each}
        </ul>
        <button class="addmodel" type="button" onclick={() => ((menu = null), (app.modelDialog = true))}><Icon name="plus" size={14} />모델 추가…</button>
      </div>
    {/if}
  </div>

  {#if chat.levels.length > 1 && model?.reasoning !== false}
    <div class="pop" use:dismiss={() => { if (menu === 'think') menu = null; }}>
      <button class="chip" type="button" title={S.thinkingLevel} aria-haspopup="listbox" aria-expanded={menu === 'think'} onclick={() => toggle('think')}>
        {chat.state?.thinkingLevel}<Icon name="down" size={12} />
      </button>
      {#if menu === 'think'}
        <div class="sheet narrow">
          <ul role="listbox" aria-label={S.thinkingLevel}>
            {#each chat.levels as l}
              <li role="option" aria-selected={l === chat.state?.thinkingLevel}>
                <button type="button" class:on={l === chat.state?.thinkingLevel} onclick={() => ((menu = null), chat.setThinking(l as ThinkingLevel))}>{l}</button>
              </li>
            {/each}
          </ul>
        </div>
      {/if}
    </div>
  {/if}

  <div class="pop" use:dismiss={() => { if (menu === 'ctx') menu = null; }}>
    <button class="chip ring" type="button" aria-label="{S.context} {pct != null ? Math.round(pct) + '%' : ''}" aria-expanded={menu === 'ctx'} onclick={() => toggle('ctx')}>
      <ContextRing percent={pct} />
    </button>
    {#if menu === 'ctx'}
      <div class="sheet ctxsheet">
        <b>{S.context}</b>
        {#if ctx && pct != null}
          <span class="pctbig">{Math.round(pct)}%</span>
          <small>{compactNumber(ctx.tokens ?? 0)} / {compactNumber(ctx.contextWindow)} {S.tokens}</small>
        {:else}
          <small>{ctx ? '압축 직후라 아직 계산되지 않았습니다.' : '모델 정보가 없습니다.'}</small>
        {/if}
        <button class="btn" type="button" disabled={chat.running || chat.compacting} onclick={() => ((menu = null), chat.compact())}>{S.compact}</button>
      </div>
    {/if}
  </div>
</div>

<style>
  .meta {
    max-width: calc(var(--measure) + 74px);
    margin-inline: auto;
  }
  .meta {
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 6px 4px 0;
    font-size: 13px;
  }
  .grow {
    flex: 1;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    height: 28px;
    padding: 0 8px;
    border-radius: 8px;
    color: var(--ink-2);
    font-weight: 520;
  }
  @media (hover: hover) {
    .chip:hover,
    .chip[aria-expanded='true'] {
      background: var(--paper-sunk);
      color: var(--ink);
    }
  }
  .chip.manual {
    color: var(--heat);
  }
  .chip.ring {
    padding: 0 6px;
  }
  .mname {
    max-width: 180px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .meta .pop ~ .pop .sheet {
    left: auto;
    right: 0;
  }
  .optlabel {
    display: grid;
    line-height: 1.35;
  }
  .ctxsheet {
    min-width: 200px;
    gap: 4px;
    padding: 12px;
  }
  .pctbig {
    font-size: 28px;
    font-weight: 750;
    letter-spacing: -0.03em;
    line-height: 1.1;
  }
  .ctxsheet .btn {
    margin-top: 8px;
    justify-content: center;
  }
  @media (max-width: 720px) {
    .meta {
      position: relative;
    }
    /* the menus open across the whole row (this rule is scoped here, so it must be undone here too) */
    .meta .pop ~ .pop .sheet {
      left: 0;
      right: 0;
    }
    .mname {
      max-width: 110px;
    }
  }
  @media (max-width: 360px) {
    .mname {
      max-width: 72px;
    }
  }
</style>
