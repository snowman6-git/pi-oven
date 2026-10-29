<script lang="ts">
  import { app } from '../lib/app.svelte';
  import { askPermission } from '../lib/notify';
  import { hasMaple } from '../lib/maple';
  import { pinRules } from '../lib/pinrules.svelte';
  import { PARTS, showcase } from '../lib/showcase.svelte';
  import { serverUrl, settings, type FontSize, type StreamFx } from '../lib/settings.svelte';

  let dlg: HTMLDialogElement;
  let server = $state(serverUrl.value);
  let replay = $state(0);

  $effect(() => dlg.showModal());

  const themes = [['dark', '다크'], ['light', '라이트'], ['auto', '시스템']] as const;
  const fonts: [FontSize, string][] = [['small', '작게'], ['normal', '보통'], ['large', '크게']];
  const fxs: { id: StreamFx; name: string; hint: string }[] = [
    { id: 'bounce', name: '튀어오름', hint: '아래에서 살짝 튀어 오르며 나타납니다.' },
    { id: 'fade', name: '페이드', hint: '투명에서 서서히 선명해집니다.' },
    { id: 'ember', name: '달궈짐', hint: '주황빛으로 달궈진 채 나타나 식으면서 제 색이 됩니다.' },
    { id: 'none', name: '없음', hint: '효과 없이 바로 표시합니다.' },
  ];

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

  function saveServer() {
    if (server.trim().replace(/\/+$/, '') === serverUrl.value) return;
    serverUrl.value = server;
    location.reload();
  }

  function toggleNotify() {
    app.toggleNotify();
    if (app.notifyOn) askPermission();
  }
</script>

<dialog bind:this={dlg} onclose={() => (app.settingsOpen = false)} aria-labelledby="st-title">
  <div class="box">
    <header>
      <h2 id="st-title">설정</h2>
      <button class="btn quiet" type="button" onclick={() => dlg.close()}>닫기</button>
    </header>

    <section>
      <h3>화면 테마</h3>
      <div class="seg" role="radiogroup" aria-label="화면 테마">
        {#each themes as [id, label]}
          <button type="button" role="radio" aria-checked={app.theme === id} class:on={app.theme === id} onclick={() => app.setTheme(id)}>{label}</button>
        {/each}
      </div>
    </section>

    <section>
      <h3>글꼴</h3>
      <div class="seg" role="radiogroup" aria-label="글꼴">
        {#if hasMaple}<button type="button" role="radio" aria-checked={settings.family === 'maple'} class:on={settings.family === 'maple'} onclick={() => settings.set('family', 'maple')} style="font-family: 'NxMaple', var(--font-ui)">메이플스토리</button>{/if}
        <button type="button" role="radio" aria-checked={settings.family === 'system'} class:on={settings.family === 'system'} onclick={() => settings.set('family', 'system')} style="font-family: system-ui, -apple-system, 'Noto Sans KR', sans-serif">시스템</button>
      </div>
    </section>

    <section>
      <h3>글자 크기</h3>
      <div class="seg" role="radiogroup" aria-label="글자 크기">
        {#each fonts as [id, label]}
          <button type="button" role="radio" aria-checked={settings.font === id} class:on={settings.font === id} onclick={() => settings.set('font', id)}>{label}</button>
        {/each}
      </div>
    </section>

    <section>
      <h3>답변이 나타나는 효과</h3>
      <div class="fx" role="radiogroup" aria-label="답변이 나타나는 효과">
        {#each fxs as f (f.id)}
          <button type="button" role="radio" aria-checked={settings.fx === f.id} class:on={settings.fx === f.id} onclick={() => pickFx(f.id)}>
            <b>{f.name}</b><small>{f.hint}</small>
          </button>
        {/each}
      </div>
      {#key replay}
        <p class="sample md" data-fx={settings.fx} aria-hidden="true">
          {#each '지금 쓰는 답변은 이런 식으로 나타납니다.'.split(' ') as w, i}
            <span class="w" style="animation-delay:{i * 90}ms">{w}</span>{' '}
          {/each}
        </p>
      {/key}
    </section>

    <section>
      <h3>동작</h3>
      <label class="row">
        <input type="checkbox" checked={settings.thinkingOpen} onchange={(e) => settings.set('thinkingOpen', e.currentTarget.checked)} />
        <span>생각 블록을 작성 중에 펼쳐서 보기</span>
      </label>
      <label class="row">
        <input type="checkbox" checked={app.notifyOn} onchange={toggleNotify} />
        <span>다른 화면에 있을 때 작업 완료와 확인 요청을 알림</span>
      </label>
    </section>

    <section>
      <h3>Pin 감지 규칙 (regex)</h3>
      <p class="note">Pin 탭이 기본으로 찾는 주소·토큰 말고, 직접 찾고 싶은 값을 정규식으로 추가합니다. 괄호 <code>( )</code>가 있으면 첫 괄호 안쪽만, 없으면 맞은 부분 전체가 Pin에 올라옵니다. 찾은 값은 토큰처럼 가려서 보이고(눈 아이콘으로 펼침), 쇼케이스의 "토큰·비밀번호·API 키"를 켜면 대화에서도 가려집니다. 이 브라우저에만 저장됩니다.</p>
      {#if pinRules.list.length}
        <ul class="rules">
          {#each pinRules.list as r (r.id)}
            <li>
              <label class="row"><input type="checkbox" checked={r.on} onchange={(e) => pinRules.toggle(r.id, e.currentTarget.checked)} /><span><b>{r.name}</b><small class="mono">{r.pattern}</small></span></label>
              <button class="btn quiet" type="button" aria-label="{r.name} 지우기" onclick={() => pinRules.remove(r.id)}>지우기</button>
            </li>
          {/each}
        </ul>
      {/if}
      <form class="newrule" onsubmit={(e) => { e.preventDefault(); addRule(); }}>
        <input class="field" bind:value={ruleName} placeholder="이름 (예: 사내 키)" maxlength="40" aria-label="규칙 이름" />
        <input class="field mono" bind:value={rulePattern} placeholder={'예: corp_[A-Za-z0-9]{20,}'} spellcheck="false" autocomplete="off" aria-label="정규식" />
        <button class="btn" type="submit">추가</button>
      </form>
      {#if ruleError}<p class="note err" role="alert">{ruleError}</p>{/if}
    </section>

    <section>
      <h3>모델</h3>
      <p class="note">다른 서버나 API의 모델을 pi에 추가합니다. 엔드포인트 주소와 키, 모델 ID를 적으면 됩니다.</p>
      <button class="btn" type="button" onclick={() => ((app.settingsOpen = false), (app.modelDialog = true))}>모델 추가·관리…</button>
    </section>

    <section>
      <h3>쇼케이스 모드</h3>
      <p class="note">화면을 공유하거나 스크린샷을 찍을 때 민감할 수 있는 것을 가립니다. 보이는 모습만 바뀌고, 저장된 내용과 복사되는 내용은 그대로입니다.</p>
      <label class="row">
        <input type="checkbox" role="switch" aria-checked={showcase.on} checked={showcase.on} onchange={(e) => showcase.setOn(e.currentTarget.checked)} />
        <span><b>쇼케이스 모드 켜기</b></span>
      </label>
      <div class="seg" role="radiogroup" aria-label="가리는 방식" class:off={!showcase.on}>
        <button type="button" role="radio" aria-checked={showcase.style === 'blur'} class:on={showcase.style === 'blur'} disabled={!showcase.on} onclick={() => showcase.setStyle('blur')}>흐리게</button>
        <button type="button" role="radio" aria-checked={showcase.style === 'example'} class:on={showcase.style === 'example'} disabled={!showcase.on} onclick={() => showcase.setStyle('example')}>예시 글자로 바꾸기</button>
      </div>
      <div class="subs" class:off={!showcase.on} role="group" aria-label="가릴 항목">
        {#each PARTS as p (p.id)}
          <label class="row sub">
            <input type="checkbox" checked={showcase.parts[p.id]} disabled={!showcase.on} onchange={(e) => showcase.set(p.id, e.currentTarget.checked)} />
            <span>{p.label}<small>{p.hint}</small></span>
          </label>
        {/each}
      </div>
    </section>

    <section>
      <h3>서버 주소</h3>
      <p class="note">비워 두면 이 페이지를 연 서버를 씁니다. 데스크톱 앱처럼 다른 곳에서 열 때만 지정하세요.</p>
      <div class="srv">
        <input class="field mono" data-priv="hosts" bind:value={server} placeholder="http://localhost:4141" spellcheck="false" aria-label="서버 주소"
          onkeydown={(e) => e.key === 'Enter' && saveServer()} />
        <button class="btn" type="button" onclick={saveServer}>적용</button>
      </div>
    </section>
  </div>
</dialog>

<style>
  dialog {
    border: 1px solid var(--rule);
    border-radius: 14px;
    background: var(--raised);
    color: var(--ink);
    padding: 0;
    width: min(560px, calc(100vw - 24px));
    max-height: calc(100dvh - 24px);
    box-shadow: var(--shadow);
  }
  dialog::backdrop {
    background: var(--scrim);
  }
  .box {
    padding: 8px 22px 22px;
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 22px;
  }
  header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    position: sticky;
    top: 0;
    padding: 14px 0 6px;
    background: var(--raised);
    z-index: 1;
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
    gap: 10px;
    align-items: center;
    padding: 5px 0;
  }
  .row input {
    accent-color: var(--accent);
    width: 16px;
    height: 16px;
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
      padding: 0 16px calc(24px + env(safe-area-inset-bottom));
    }
    header {
      padding-top: calc(14px + env(safe-area-inset-top));
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
</style>
