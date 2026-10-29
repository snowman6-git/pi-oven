export type StreamFx = 'bounce' | 'fade' | 'ember' | 'none';
export type FontSize = 'small' | 'normal' | 'large';
export type FontFamily = 'maple' | 'system';

const KEY = 'pi-oven.settings';

interface Saved { fx: StreamFx; font: FontSize; family: FontFamily; thinkingOpen: boolean }
const defaults: Saved = { fx: 'bounce', font: 'normal', family: 'maple', thinkingOpen: true };

function read(): Saved {
  try {
    const v = { ...defaults, ...JSON.parse(localStorage.getItem(KEY) ?? '{}') };
    // A saved value this version does not know (an older font choice) means the system font.
    if (v.family !== 'maple' && v.family !== 'system') v.family = 'system';
    return v;
  } catch { return defaults; }
}

class Settings {
  #s = $state<Saved>(read());

  get fx() { return this.#s.fx; }
  get font() { return this.#s.font; }
  get family() { return this.#s.family; }
  get thinkingOpen() { return this.#s.thinkingOpen; }

  set<K extends keyof Saved>(key: K, value: Saved[K]) {
    this.#s[key] = value;
    try { localStorage.setItem(KEY, JSON.stringify(this.#s)); } catch { /* private mode */ }
    if (key === 'font' || key === 'family') this.apply();
  }

  apply() {
    const px = { small: '14px', normal: '15px', large: '16.5px' }[this.#s.font];
    const root = document.documentElement.style;
    root.setProperty('--fs', px);
    // Maple is the default and what is not in it falls to the system font; the other choice is the system font alone.
    if (this.#s.family === 'system') root.setProperty('--font-ui', "system-ui, -apple-system, 'Apple SD Gothic Neo', 'Noto Sans KR', 'Malgun Gothic', 'Segoe UI', sans-serif");
    else root.removeProperty('--font-ui');
  }
}

export const settings = new Settings();

// Where the API lives. Empty means "the server that served this page"; a Tauri build points it elsewhere.
const SERVER_KEY = 'pi-oven.server';
export const serverUrl = {
  get value(): string {
    try { return (localStorage.getItem(SERVER_KEY) ?? '').replace(/\/+$/, ''); } catch { return ''; }
  },
  set value(v: string) {
    try { v.trim() ? localStorage.setItem(SERVER_KEY, v.trim().replace(/\/+$/, '')) : localStorage.removeItem(SERVER_KEY); } catch { /* private mode */ }
  },
};
