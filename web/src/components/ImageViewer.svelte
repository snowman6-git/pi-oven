<script lang="ts">
  import { t } from '../lib/i18n.svelte';
  import { app } from '../lib/app.svelte';
  import { S } from '../lib/strings';
  import { compactNumber } from '../lib/util';
  import Icon from './Icon.svelte';

  // A picture from the conversation, large: drag or touch to move it, wheel / pinch / double click to enlarge it. The
  // bar drags the window itself (on a wide screen; on a phone the window is the screen).
  let { images, start }: { images: { mimeType: string; data: string }[]; start: number } = $props();

  let dlg = $state<HTMLDialogElement>();
  let box = $state<HTMLElement>();
  let stage = $state<HTMLElement>();
  let i = $state(0);
  $effect.pre(() => { i = start; });
  const img = $derived(images[i]);
  const bytes = $derived(Math.floor((img.data.length * 3) / 4) - (img.data.endsWith('==') ? 2 : img.data.endsWith('=') ? 1 : 0));

  let nat = $state<{ w: number; h: number } | null>(null);
  let broken = $state(false);
  let view = $state({ w: 0, h: 0 });
  /** On top of "fits the window": how much larger, and how far moved (px from the middle). */
  let scale = $state(1);
  let x = $state(0);
  let y = $state(0);
  let dragging = $state(false);
  /** The window's own place, from the middle of the screen. */
  let wx = $state(0);
  let wy = $state(0);
  /** Fills the screen: the browser's own full screen where it has one, else the whole page. */
  let full = $state(false);
  let realFull = $state(false);
  const canFull = document.fullscreenEnabled;

  // never enlarged to fit: a small icon stays its own size
  const fit = $derived(nat && view.w ? Math.min(1, view.w / nat.w, view.h / nat.h) : 1);
  const shown = $derived(fit * scale);
  const MAX = $derived(Math.max(8, 8 / fit));

  $effect(() => { if (dlg && !dlg.open) dlg.showModal(); });
  $effect(() => {
    if (!stage) return;
    const ro = new ResizeObserver(() => { view = { w: stage!.clientWidth - 24, h: stage!.clientHeight - 24 }; settle(); });
    ro.observe(stage);
    return () => ro.disconnect();
  });

  function close() {
    if (document.fullscreenElement) void document.exitFullscreen().catch(() => {});
    app.viewer = null;
  }
  function reset() { scale = 1; x = 0; y = 0; }
  function go(d: number) {
    if (images.length < 2) return;
    i = (i + d + images.length) % images.length;
    nat = null; broken = false; reset();
  }
  /** A corner of the picture always stays in sight. */
  function settle() {
    if (!nat) return;
    const mx = (nat.w * shown + view.w) / 2 - 60, my = (nat.h * shown + view.h) / 2 - 60;
    x = Math.max(-mx, Math.min(mx, x));
    y = Math.max(-my, Math.min(my, y));
  }
  /** Enlarge around a point of the stage (from its middle), which stays where it is. */
  function zoom(to: number, px = 0, py = 0) {
    const s = Math.max(Math.min(1, 0.2 / fit), Math.min(MAX, to));
    x = px - ((px - x) * s) / scale;
    y = py - ((py - y) * s) / scale;
    scale = s;
    settle();
  }
  const at = (e: { clientX: number; clientY: number }) => {
    const r = stage!.getBoundingClientRect();
    return { px: e.clientX - r.left - r.width / 2, py: e.clientY - r.top - r.height / 2 };
  };

  // one finger or the mouse moves, two fingers enlarge and move together
  const down = new Map<number, { px: number; py: number }>();
  function press(e: PointerEvent) {
    if (e.button > 0) return;
    stage!.setPointerCapture(e.pointerId);
    down.set(e.pointerId, at(e));
    dragging = true;
  }
  function move(e: PointerEvent) {
    const was = down.get(e.pointerId);
    if (!was) return;
    const now = at(e);
    if (down.size === 1) {
      x += now.px - was.px;
      y += now.py - was.py;
      settle();
    } else if (down.size === 2) {
      const other = [...down].find(([id]) => id !== e.pointerId)![1];
      const d0 = Math.hypot(was.px - other.px, was.py - other.py), d1 = Math.hypot(now.px - other.px, now.py - other.py);
      const m0 = { px: (was.px + other.px) / 2, py: (was.py + other.py) / 2 }, m1 = { px: (now.px + other.px) / 2, py: (now.py + other.py) / 2 };
      x += m1.px - m0.px;
      y += m1.py - m0.py;
      if (d0 > 8) zoom((scale * d1) / d0, m1.px, m1.py);
    }
    down.set(e.pointerId, now);
  }
  let tapAt = 0, tapX = 0, tapY = 0;
  function lift(e: PointerEvent) {
    down.delete(e.pointerId);
    dragging = down.size > 0;
    if (e.type !== 'pointerup' || down.size) return;
    // a double tap or click: up to its own size (or 2.5x when it already is), and back
    const near = Math.hypot(e.clientX - tapX, e.clientY - tapY) < 24;
    if (e.timeStamp - tapAt < 320 && near) {
      const p = at(e);
      if (scale > 1.05) reset();
      else zoom(fit < 0.8 ? 1 / fit : 2.5, p.px, p.py);
      tapAt = 0;
    } else { tapAt = e.timeStamp; tapX = e.clientX; tapY = e.clientY; }
  }
  function wheel(e: WheelEvent) {
    e.preventDefault();
    const p = at(e);
    zoom(scale * Math.exp(-Math.max(-60, Math.min(60, e.deltaY)) * (e.ctrlKey ? 0.012 : 0.004)), p.px, p.py);
  }
  const onWheel = (node: HTMLElement) => { node.addEventListener('wheel', wheel, { passive: false }); };

  // the window, by its bar
  function grab(e: PointerEvent) {
    if (full || app.narrow || e.button > 0 || (e.target as HTMLElement).closest('button')) return;
    const bar = e.currentTarget as HTMLElement;
    bar.setPointerCapture(e.pointerId);
    const ox = e.clientX - wx, oy = e.clientY - wy;
    const mv = (m: PointerEvent) => {
      const r = box!.getBoundingClientRect(), mx = (innerWidth + r.width) / 2 - 120, my = (innerHeight - r.height) / 2;
      wx = Math.max(-mx, Math.min(mx, m.clientX - ox));
      // the bar stays on the screen
      wy = Math.max(-my, Math.min(my + r.height - 60, m.clientY - oy));
    };
    const up = () => { bar.removeEventListener('pointermove', mv); bar.removeEventListener('pointerup', up); bar.removeEventListener('pointercancel', up); };
    bar.addEventListener('pointermove', mv);
    bar.addEventListener('pointerup', up);
    bar.addEventListener('pointercancel', up);
  }

  async function toggleFull() {
    if (full) {
      if (document.fullscreenElement) await document.exitFullscreen().catch(() => {});
      full = false;
      return;
    }
    full = true;
    if (canFull) await box?.requestFullscreen().catch(() => {});
  }
  function fullChanged() {
    realFull = !!document.fullscreenElement && document.fullscreenElement === box;
    if (!document.fullscreenElement) full = false; // left with Escape or the phone's back
  }

  function key(e: KeyboardEvent) {
    const step = 60;
    const done = () => e.preventDefault();
    if (e.key === '+' || e.key === '=') { zoom(scale * 1.25); done(); }
    else if (e.key === '-') { zoom(scale / 1.25); done(); }
    else if (e.key === '0') { reset(); done(); }
    else if (e.key === 'f') { void toggleFull(); done(); }
    else if (e.key.startsWith('Arrow') && (e.target as HTMLElement).closest?.('.stage')) {
      const side = e.key === 'ArrowLeft' ? 1 : e.key === 'ArrowRight' ? -1 : 0;
      if (side && scale <= 1.05) go(-side);
      else { x += side * step; y += (e.key === 'ArrowUp' ? 1 : e.key === 'ArrowDown' ? -1 : 0) * step; settle(); }
      done();
    }
  }
</script>

<svelte:document onfullscreenchange={fullChanged} />

<dialog bind:this={dlg} class:full aria-label={t('viewer.title')} oncancel={(e) => { e.preventDefault(); if (full && !realFull) full = false; else close(); }} onclose={close} onkeydown={key}
  style:translate={full || app.narrow ? undefined : `${wx}px ${wy}px`}>
  <div class="box" bind:this={box}>
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="bar" class:grabs={!full && !app.narrow} onpointerdown={grab}>
      {#if images.length > 1}
        <button class="icon-btn" type="button" aria-label={t('viewer.prev')} title={t('viewer.prev')} onclick={() => go(-1)}><Icon name="left" /></button>
        <span class="n">{i + 1} / {images.length}</span>
        <button class="icon-btn" type="button" aria-label={t('viewer.next')} title={t('viewer.next')} onclick={() => go(1)}><Icon name="right" /></button>
      {/if}
      <span class="grow"></span>
      {#if canFull || !app.narrow}
        <button class="icon-btn" type="button" aria-pressed={full} aria-label={full ? t('viewer.fullOff') : t('viewer.fullOn')} title={full ? t('viewer.fullOff') : t('viewer.fullOn')} onclick={toggleFull}><Icon name={full ? 'collapse' : 'expand'} /></button>
      {/if}
      <button class="icon-btn" type="button" aria-label={S.close} title={S.close} onclick={close}><Icon name="x" /></button>
    </div>

    <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
    <div class="stage" class:dragging role="group" aria-label={t('viewer.hint')} tabindex="0" bind:this={stage} data-priv="files"
      onpointerdown={press} onpointermove={move} onpointerup={lift} onpointercancel={lift} use:onWheel>
      {#if broken}
        <p>{t('editor.imageBroken')}</p>
      {:else}
        <img alt="" draggable="false" src={`data:${img.mimeType};base64,${img.data}`}
          style:width={nat ? `${nat.w * fit}px` : undefined} style:transform={`translate(${x}px, ${y}px) scale(${scale})`}
          onload={(e) => { const im = e.currentTarget as HTMLImageElement; nat = { w: im.naturalWidth, h: im.naturalHeight }; }} onerror={() => (broken = true)} />
      {/if}
    </div>

    <div class="foot">
      {#if nat}<span>{nat.w} × {nat.h}</span>{/if}
      <span>{compactNumber(bytes)}B</span>
      <span class="grow"></span>
      <button class="pct" type="button" title={t('viewer.reset')} onclick={reset}>{Math.round(shown * 100)}%</button>
    </div>
  </div>
</dialog>

<style>
  dialog {
    width: min(1080px, calc(100vw - 48px));
    height: min(820px, calc(100dvh - 48px));
    max-width: none;
    max-height: none;
    padding: 0;
    border: 1px solid var(--rule);
    border-radius: 14px;
    background: var(--raised);
    color: var(--ink);
    box-shadow: var(--shadow);
    overflow: hidden;
  }
  dialog::backdrop {
    background: var(--scrim);
  }
  dialog.full {
    width: 100vw;
    height: 100dvh;
    border: 0;
    border-radius: 0;
  }
  .box {
    height: 100%;
    display: flex;
    flex-direction: column;
    min-height: 0;
    background: var(--raised);
  }
  .bar,
  .foot {
    flex: none;
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 8px;
  }
  .bar.grabs {
    cursor: grab;
    touch-action: none;
    user-select: none;
  }
  .bar.grabs:active {
    cursor: grabbing;
  }
  .grow {
    flex: 1;
  }
  .n {
    font: 12.5px var(--font-mono);
    color: var(--ink-2);
  }
  .stage {
    flex: 1;
    min-height: 0;
    position: relative;
    display: grid;
    place-items: center;
    overflow: hidden;
    cursor: grab;
    touch-action: none;
    user-select: none;
    contain: strict;
    border-block: 1px solid var(--rule);
    /* a checkerboard, so transparent parts read as transparent */
    background: repeating-conic-gradient(var(--paper-sunk) 0% 25%, var(--paper) 0% 50%) 50% / 20px 20px;
  }
  .stage.dragging {
    cursor: grabbing;
  }
  .stage:focus-visible {
    outline-offset: -3px;
  }
  .stage p {
    color: var(--ink-2);
  }
  img {
    grid-area: 1 / 1;
    max-width: none;
    height: auto;
    box-shadow: 0 0 0 1px var(--rule);
    pointer-events: none;
  }
  .foot {
    gap: 14px;
    padding: 6px 14px;
    font-size: 12.5px;
    color: var(--ink-3);
    font-variant-numeric: tabular-nums;
  }
  .pct {
    font: 12.5px var(--font-mono);
    color: var(--ink-2);
    padding: 3px 8px;
    border-radius: 6px;
  }
  @media (hover: hover) {
    .pct:hover {
      background: var(--rule);
      color: var(--ink);
    }
  }
  @media (max-width: 720px) {
    dialog {
      width: 100vw;
      height: 100dvh;
      border: 0;
      border-radius: 0;
    }
    .bar .icon-btn {
      width: 40px;
      height: 40px;
    }
  }
</style>
