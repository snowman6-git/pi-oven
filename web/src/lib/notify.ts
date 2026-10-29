// Tells the person a run finished or needs an answer while they are in another tab or session.
// Works on plain HTTP (tab title + a short tone); desktop notifications need https or localhost.

const KEY = 'pi-oven.notify';

export const notifyEnabled = (): boolean => {
  try { return localStorage.getItem(KEY) !== 'off'; } catch { return true; }
};
export function setNotifyEnabled(on: boolean) {
  try { localStorage.setItem(KEY, on ? 'on' : 'off'); } catch { /* private mode */ }
}

/** Call from a click handler: browsers only grant permission on a user gesture. */
export function askPermission() {
  if (typeof Notification !== 'undefined' && Notification.permission === 'default') void Notification.requestPermission();
}

let audio: AudioContext | undefined;
const DROPS: Record<'bg-start' | 'bg-card' | 'bg-stop' | 'bg-done' | 'bg-bubble', [number, number, number, number]> = {
  'bg-start': [1150, 500, 0.16, 0.10], // object born: small and bright
  'bg-card': [950, 420, 0.2, 0.11], // the card lands in the tool row
  'bg-done': [820, 240, 0.34, 0.14], // finished: the big one
  'bg-stop': [320, 130, 0.22, 0.12], // killed: dull thud
  'bg-bubble': [1400, 900, 0.12, 0.08], // its notice bubble enters the chat: a small, high pop
};
export type Beep = 'done' | 'ask' | 'todo-step' | keyof typeof DROPS;
const BEEP_HZ: Record<'done' | 'ask' | 'todo-step', number> = { done: 880, ask: 660, 'todo-step': 523 };
function drop(ctx: AudioContext, f0: number, f1: number, dur: number, peak: number, t0: number) {
  const o = ctx.createOscillator();
  const g = ctx.createGain();
  o.frequency.setValueAtTime(f0, t0);
  o.frequency.exponentialRampToValueAtTime(f1, t0 + dur * 0.75);
  g.gain.setValueAtTime(0.0001, t0);
  g.gain.exponentialRampToValueAtTime(peak, t0 + 0.006);
  g.gain.exponentialRampToValueAtTime(0.0001, t0 + dur);
  o.connect(g).connect(ctx.destination);
  o.start(t0);
  o.stop(t0 + dur + 0.02);
  const p = ctx.createOscillator();
  const pg = ctx.createGain();
  p.frequency.setValueAtTime(f0 * 2.76, t0);
  pg.gain.setValueAtTime(0.0001, t0);
  pg.gain.exponentialRampToValueAtTime(peak * 0.3, t0 + 0.004);
  pg.gain.exponentialRampToValueAtTime(0.0001, t0 + dur * 0.4);
  p.connect(pg).connect(ctx.destination);
  p.start(t0);
  p.stop(t0 + dur * 0.4 + 0.02);
}
function tone(kind: Beep, step = 0) {
  try {
    audio ??= new AudioContext();
    if (audio.state === 'suspended') void audio.resume();
    const t0 = audio.currentTime;
    if (kind === 'bg-start' || kind === 'bg-card' || kind === 'bg-stop' || kind === 'bg-done' || kind === 'bg-bubble') {
      // a water drop: a sine that falls in pitch as it sinks, plus one quiet inharmonic partial for the "plink"
      drop(audio, ...DROPS[kind], t0);
      return;
    }
    const o = audio.createOscillator();
    const g = audio.createGain();
    o.frequency.value = kind === 'todo-step' ? BEEP_HZ['todo-step'] * Math.pow(2, Math.min(step, 10) / 12) : BEEP_HZ[kind]; // a fixed 10-step ladder
    g.gain.setValueAtTime(0.0001, t0);
    g.gain.exponentialRampToValueAtTime(0.12, t0 + 0.02);
    g.gain.exponentialRampToValueAtTime(0.0001, t0 + 0.35);
    o.connect(g).connect(audio.destination);
    o.start(t0);
    o.stop(t0 + 0.4);
  } catch { /* autoplay blocked */ }
}

/** State-driven sounds (background tasks, todo steps): respect the notification-sound switch. */
export function beep(kind: Beep, step = 0) {
  if (notifyEnabled()) tone(kind, step);
}

export function alertPerson(kind: 'done' | 'ask', title: string, onClick: () => void) {
  if (!notifyEnabled()) return;
  tone(kind);
  if (typeof Notification !== 'undefined' && Notification.permission === 'granted') {
    const n = new Notification(kind === 'ask' ? '확인이 필요합니다' : '작업이 끝났습니다', { body: title, tag: 'pi-oven-' + kind });
    n.onclick = () => { window.focus(); onClick(); n.close(); };
  }
}
