<script lang="ts">
  import { app } from '../lib/app.svelte';
  import { serverUrl } from '../lib/settings.svelte';
  import { S } from '../lib/strings';
  import Icon from './Icon.svelte';

  let addr = $state(serverUrl.value);
  // Only the app (desktop or phone) sets this: the way back to choosing another server.
  const back = (window as unknown as { __piOvenBack?: () => void }).__piOvenBack;

  // A new address is tried in place; there is no reload to land on the same screen again.
  function apply() {
    serverUrl.value = addr;
    void app.boot();
  }
  function useThisPage() {
    addr = '';
    apply();
  }
</script>

<main>
  {#if back}
    <button type="button" class="icon-btn back" onclick={back} aria-label={S.tokenBack} title={S.tokenBack}><Icon name="left" size={20} /></button>
  {/if}
  <div class="box">
    <p>{S.serverDown}</p>
    <p class="faint mono" data-priv="hosts">{serverUrl.value || S.serverThisPage}</p>
    {#if app.bootError}<p class="faint mono err" data-priv="hosts">{app.bootError}</p>{/if}
    <button class="btn" type="button" onclick={() => app.boot()}>{S.retry}</button>
    <form onsubmit={(e) => { e.preventDefault(); apply(); }}>
      <input class="field mono" data-priv="hosts" bind:value={addr} placeholder="http://localhost:4141" spellcheck="false" aria-label="서버 주소" />
      <button class="btn primary" type="submit">{S.serverChange}</button>
    </form>
    {#if serverUrl.value}
      <button class="btn quiet" type="button" onclick={useThisPage}>{S.serverReset}</button>
    {/if}
  </div>
</main>

<style>
  main {
    height: 100%;
    display: grid;
    place-items: center;
    padding: 24px;
    position: relative;
    color: var(--ink-2);
  }
  .box {
    width: min(400px, 100%);
    display: grid;
    gap: 12px;
    justify-items: stretch;
  }
  p {
    margin: 0;
    overflow-wrap: anywhere;
  }
  .faint {
    color: var(--ink-3);
    font-size: 13px;
  }
  .err {
    color: var(--bad);
  }
  form {
    display: flex;
    gap: 8px;
  }
  .btn {
    justify-content: center;
    white-space: nowrap;
  }
  .back {
    position: absolute;
    top: 12px;
    left: 12px;
    width: 40px;
    height: 40px;
  }
</style>
