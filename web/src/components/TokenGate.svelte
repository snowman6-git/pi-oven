<script lang="ts">
  import { app } from '../lib/app.svelte';
  import { S } from '../lib/strings';
  import Icon from './Icon.svelte';
  let token = $state('');
  // Only the app (desktop or phone) sets this: the way back to choosing another server.
  const back = (window as unknown as { __piOvenBack?: () => void }).__piOvenBack;
</script>

<main>
  {#if back}
    <button type="button" class="icon-btn back" onclick={back} aria-label={S.tokenBack} title={S.tokenBack}><Icon name="left" size={20} /></button>
  {/if}
  <form onsubmit={(e) => { e.preventDefault(); void app.submitToken(token); }}>
    <h1><img src="/logo.svg" alt="" width="44" height="44" />pi-oven</h1>
    <h2>{S.tokenTitle}</h2>
    <p>{S.tokenHelp}</p>
    <input class="field mono" type="password" autocomplete="off" bind:value={token} aria-label="토큰 또는 비밀번호" placeholder="토큰 또는 비밀번호" aria-invalid={app.error === 'wrong'} />
    {#if app.error === 'wrong'}<p class="err" role="alert">{S.tokenWrong}</p>{/if}
    {#if app.error === 'locked'}<p class="err" role="alert">{S.tokenLocked}</p>{/if}
    <button class="btn primary" type="submit" disabled={!token.trim()}>{S.tokenSubmit}</button>
  </form>
</main>

<style>
  main {
    height: 100%;
    display: grid;
    place-items: center;
    padding: 24px;
    position: relative;
  }
  .back {
    position: absolute;
    top: 12px;
    left: 12px;
    width: 40px;
    height: 40px;
  }
  form {
    width: min(400px, 100%);
    display: grid;
    gap: 12px;
  }
  h1 {
    margin: 0 0 16px;
    font-size: 44px;
    letter-spacing: -0.05em;
    line-height: 1;
  }
  h1 {
    display: flex;
    align-items: center;
    gap: 12px;
  }
  h2 {
    margin: 0;
    font-size: 20px;
  }
  p {
    margin: 0;
    color: var(--ink-2);
  }
  .err {
    color: var(--bad);
  }
  .btn {
    justify-content: center;
    height: 38px;
  }
</style>
