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
function tone(kind: 'done' | 'ask') {
  try {
    audio ??= new AudioContext();
    const o = audio.createOscillator();
    const g = audio.createGain();
    o.frequency.value = kind === 'ask' ? 660 : 880;
    g.gain.setValueAtTime(0.0001, audio.currentTime);
    g.gain.exponentialRampToValueAtTime(0.12, audio.currentTime + 0.02);
    g.gain.exponentialRampToValueAtTime(0.0001, audio.currentTime + 0.35);
    o.connect(g).connect(audio.destination);
    o.start();
    o.stop(audio.currentTime + 0.4);
  } catch { /* autoplay blocked */ }
}

export function alertPerson(kind: 'done' | 'ask', title: string, onClick: () => void) {
  if (!notifyEnabled()) return;
  tone(kind);
  if (typeof Notification !== 'undefined' && Notification.permission === 'granted') {
    const n = new Notification(kind === 'ask' ? '확인이 필요합니다' : '작업이 끝났습니다', { body: title, tag: 'pi-oven-' + kind });
    n.onclick = () => { window.focus(); onClick(); n.close(); };
  }
}
