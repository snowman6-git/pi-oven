// Showcase mode: hide what should not be on a screenshot or a shared screen. One switch for the mode, and one for each
// kind of thing, so a demo can show the file tree and still hide the folder names. The page is changed only in how it
// looks (blurred, or text replaced as it is drawn); nothing is stored differently.

import { pinRules } from './pinrules.svelte';
import { scrub, type ScrubOpts } from './scrub';

export type Style = 'blur' | 'example';

export type Part = 'paths' | 'user' | 'titles' | 'files' | 'secrets' | 'hosts' | 'chat';

export const PARTS: { id: Part; label: string; hint: string; on: boolean }[] = [
  { id: 'paths', label: '프로젝트 폴더 경로', hint: '폴더 이름과 경로(사이드바, 헤더, 정보 패널)를 흐리게, 대화와 출력 속의 경로는 <프로젝트>로 바꿉니다.', on: true },
  { id: 'user', label: '사용자 이름·홈 폴더', hint: '/home/이름, C:\\Users\\이름 같은 부분을 ~ 나 user 로 바꿉니다.', on: true },
  { id: 'titles', label: '세션 제목·첫 메시지', hint: '목록의 세션 제목, 헤더 제목, 트리의 메시지 미리보기를 흐리게 합니다.', on: true },
  { id: 'secrets', label: '토큰·비밀번호·API 키', hint: '대화 속에서 찾은 토큰과 키, 주소에 붙은 token= 값을 가립니다. Pin 탭의 값도 가립니다.', on: true },
  { id: 'hosts', label: '서버 주소·IP', hint: '주소의 호스트는 server로, IP는 •.•.•.• 로 바꾸고 설정·모델 화면의 주소를 흐리게 합니다.', on: true },
  { id: 'files', label: '파일 이름', hint: '탐색기, 편집기 탭, Git·Diff 목록의 파일 이름을 흐리게 합니다.', on: false },
  { id: 'chat', label: '대화 본문', hint: '메시지와 답변 글 전체를 흐리게 합니다(가장 강한 설정).', on: false },
];

const KEY = 'pi-oven.showcase';

function load(): { on: boolean; style: Style; parts: Record<Part, boolean> } {
  const parts = Object.fromEntries(PARTS.map((p) => [p.id, p.on])) as Record<Part, boolean>;
  try {
    const v = JSON.parse(localStorage.getItem(KEY) ?? 'null');
    if (v && typeof v === 'object') {
      for (const p of PARTS) if (typeof v.parts?.[p.id] === 'boolean') parts[p.id] = v.parts[p.id];
      return { on: v.on === true, style: v.style === 'example' ? 'example' : 'blur', parts };
    }
  } catch { /* private mode, or something else wrote here */ }
  return { on: false, style: 'blur', parts };
}

class Showcase {
  on = $state(false);
  /** How a hidden thing is drawn: blurred, or overwritten with example text. */
  style = $state<Style>('blur');
  parts = $state<Record<Part, boolean>>(Object.fromEntries(PARTS.map((p) => [p.id, p.on])) as Record<Part, boolean>);
  /** Folders and the home folder as this machine knows them (set by the app). */
  cwds = $state.raw<string[]>([]);
  home = $state('');

  constructor() {
    const v = load();
    this.on = v.on;
    this.style = v.style;
    this.parts = v.parts;
  }

  #save() {
    try { localStorage.setItem(KEY, JSON.stringify({ on: this.on, style: this.style, parts: this.parts })); } catch { /* private mode */ }
  }

  setOn(on: boolean) {
    this.on = on;
    this.#save();
  }

  setStyle(style: Style) {
    this.style = style;
    this.#save();
  }

  set(part: Part, on: boolean) {
    this.parts[part] = on;
    this.#save();
  }

  active(part: Part) {
    return this.on && this.parts[part];
  }

  /** The kinds currently hidden, for the page's stylesheet (`[data-showcase~="files"]`). */
  get attr() {
    return this.on ? PARTS.filter((p) => this.parts[p.id]).map((p) => p.id).join(' ') : '';
  }

  /** The kinds whose text is overwritten (example style), for lib/maskdom. */
  get masked(): Part[] {
    return this.on && this.style === 'example' ? PARTS.filter((p) => this.parts[p.id]).map((p) => p.id) : [];
  }

  /** Text as it should be drawn: unchanged when the mode is off. */
  text = (s: string): string => {
    if (!this.on || !s) return s;
    const o: ScrubOpts = { cwds: this.cwds, home: this.home, paths: this.parts.paths, user: this.parts.user, secrets: this.parts.secrets, hosts: this.parts.hosts, rules: pinRules.compiled };
    return o.paths || o.user || o.secrets || o.hosts ? scrub(s, o) : s;
  };

  /** A tooltip that does not give away what the blurred text beside it hides. */
  tip = (part: Part, s: string | undefined): string | undefined => (this.active(part) ? undefined : s);
}

export const showcase = new Showcase();
