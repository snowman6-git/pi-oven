// pi's own slash commands (/new, /model, ...) are handled by its terminal UI and do nothing when sent
// through RPC. The ones with an RPC equivalent are mapped here; the rest explain themselves.
import { app } from './app.svelte';
import { copyText } from './clipboard';
import type { Chat } from './chat.svelte';
import type { ThinkingLevel } from './protocol';

export interface Builtin { name: string; description: string; run: (chat: Chat, arg: string) => void | Promise<void> }

const say = (chat: Chat, text: string) => chat.notice('info', text);

export const builtins: Builtin[] = [
  { name: 'new', description: '새 세션 시작', run: (c) => c.newSession() },
  { name: 'name', description: '세션 이름 지정: /name 이름', run: (c, a) => (a ? c.rename(a) : say(c, c.title || '이름이 없습니다.')) },
  { name: 'compact', description: '컨텍스트 압축: /compact [지침]', run: (c, a) => c.compact(a || undefined) },
  {
    name: 'model', description: '모델 선택: /model 이름',
    run: (c, a) => {
      const q = a.toLowerCase();
      const hit = c.models.find((m) => `${m.provider}/${m.id}`.toLowerCase() === q || m.id.toLowerCase() === q)
        ?? c.models.find((m) => `${m.provider}/${m.id} ${m.name ?? ''}`.toLowerCase().includes(q));
      return hit && q ? c.setModel(hit) : say(c, `모델: ${c.models.map((m) => m.id).join(', ')}`);
    },
  },
  {
    name: 'thinking', description: '추론 강도: /thinking 단계',
    run: (c, a) => (c.levels.includes(a as ThinkingLevel) ? c.setThinking(a as ThinkingLevel) : say(c, `단계: ${c.levels.join(', ')}`)),
  },
  {
    name: 'clone', description: '현재 세션 복제',
    run: async (c) => {
      try { await c.conn.request({ type: 'clone' }); await c.load(); } catch (e) { c.notice('error', (e as Error).message); }
    },
  },
  {
    name: 'export', description: '세션을 HTML로 내보내기',
    run: async (c) => {
      try {
        const r = await c.conn.request<{ path: string }>({ type: 'export_html' });
        say(c, `저장됨: ${r.path}`);
      } catch (e) { c.notice('error', (e as Error).message); }
    },
  },
  {
    name: 'copy', description: '마지막 답변 복사',
    run: async (c) => {
      const r = await c.conn.request<{ text: string | null }>({ type: 'get_last_assistant_text' });
      if (r.text) c.toast((await copyText(r.text)) ? '복사했습니다' : '복사하지 못했습니다', 'info');
    },
  },
  {
    name: 'session', description: '세션 정보',
    run: async (c) => {
      await c.refreshStats();
      const s = c.stats;
      say(c, s ? `요청 ${s.userMessages}, 응답 ${s.assistantMessages}, 도구 ${s.toolCalls}, 토큰 ${s.tokens.total.toLocaleString()}, 비용 $${s.cost.toFixed(3)}` : '정보가 없습니다.');
    },
  },
  { name: 'reload', description: '확장·스킬 다시 불러오기(pi 재시작)', run: (c) => app.restartChat(c) },
  { name: 'quit', description: '이 세션 닫기', run: (c) => app.closeChat(c) },
];

const unsupported: Record<string, string> = {
  resume: '왼쪽 목록에서 세션을 고르세요.',
  fork: '메시지에 마우스를 올리면 나오는 "여기서 분기"를 쓰세요.',
  tree: '세션 트리는 아직 웹에서 지원하지 않습니다.',
  settings: '설정은 터미널의 pi에서 바꿀 수 있습니다.',
  login: '로그인은 터미널의 pi에서 하세요.',
  logout: '로그아웃은 터미널의 pi에서 하세요.',
  trust: '프로젝트 신뢰는 터미널의 pi에서 정하세요. (서버 옵션 --pi-arg=--approve도 가능)',
  hotkeys: '웹에서는 해당 없음.',
  share: '웹에서는 지원하지 않습니다. /export를 쓰세요.',
};

/** Returns true when the text was a terminal-only command and has been dealt with. */
export function handleSlash(chat: Chat, text: string): boolean {
  const m = text.match(/^\/([\w:-]+)\s*(.*)$/s);
  if (!m) return false;
  const [, name, arg] = m;
  const b = builtins.find((x) => x.name === name);
  if (b) {
    void b.run(chat, arg.trim());
    return true;
  }
  if (name in unsupported) {
    chat.notice('info', `/${name}: ${unsupported[name]}`);
    return true;
  }
  return false;
}
