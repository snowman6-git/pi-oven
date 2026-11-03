// pi's own slash commands (/new, /model, ...) are handled by its terminal UI and do nothing when sent
// through RPC. The ones with an RPC equivalent are mapped here; the rest explain themselves.
import { app } from './app.svelte';
import { copyText } from './clipboard';
import type { Chat } from './chat.svelte';
import type { ThinkingLevel } from './protocol';
import { t, type Key } from './i18n';

export interface Builtin { name: string; description: string; run: (chat: Chat, arg: string) => void | Promise<void> }

const say = (chat: Chat, text: string) => chat.notice('info', text);

export const builtins: Builtin[] = [
  { name: 'new', get description() { return t('slash.desc.new'); }, run: (c) => c.newSession() },
  { name: 'name', get description() { return t('slash.desc.name'); }, run: (c, a) => (a ? c.rename(a) : say(c, c.title || t('slash.noName'))) },
  { name: 'compact', get description() { return t('slash.desc.compact'); }, run: (c, a) => c.compact(a || undefined) },
  {
    name: 'model', get description() { return t('slash.desc.model'); },
    run: (c, a) => {
      const q = a.toLowerCase();
      const hit = c.models.find((m) => `${m.provider}/${m.id}`.toLowerCase() === q || m.id.toLowerCase() === q)
        ?? c.models.find((m) => `${m.provider}/${m.id} ${m.name ?? ''}`.toLowerCase().includes(q));
      return hit && q ? c.setModel(hit) : say(c, t('slash.models', { list: c.models.map((m) => m.id).join(', ') }));
    },
  },
  {
    name: 'thinking', get description() { return t('slash.desc.thinking'); },
    run: (c, a) => (c.levels.includes(a as ThinkingLevel) ? c.setThinking(a as ThinkingLevel) : say(c, t('slash.levels', { list: c.levels.join(', ') }))),
  },
  {
    name: 'clone', get description() { return t('slash.desc.clone'); },
    run: async (c) => {
      try { await c.conn.request({ type: 'clone' }); await c.load(); } catch (e) { c.notice('error', (e as Error).message); }
    },
  },
  {
    name: 'export', get description() { return t('slash.desc.export'); },
    run: async (c) => {
      try {
        const r = await c.conn.request<{ path: string }>({ type: 'export_html' });
        say(c, t('slash.saved', { path: r.path }));
      } catch (e) { c.notice('error', (e as Error).message); }
    },
  },
  {
    name: 'copy', get description() { return t('slash.desc.copy'); },
    run: async (c) => {
      const r = await c.conn.request<{ text: string | null }>({ type: 'get_last_assistant_text' });
      if (r.text) c.toast((await copyText(r.text)) ? t('appmsg.copied') : t('appmsg.copyFailed'), 'info');
    },
  },
  {
    name: 'session', get description() { return t('slash.desc.session'); },
    run: async (c) => {
      await c.refreshStats();
      const s = c.stats;
      say(c, s ? t('slash.stats', { req: s.userMessages, res: s.assistantMessages, tools: s.toolCalls, tokens: s.tokens.total.toLocaleString(), cost: s.cost.toFixed(3) }) : t('slash.noStats'));
    },
  },
  { name: 'reload', get description() { return t('slash.desc.reload'); }, run: (c) => app.restartChat(c) },
  { name: 'quit', get description() { return t('slash.desc.quit'); }, run: (c) => app.closeChat(c) },
];

const unsupported: Record<string, string> = {
  resume: '',
  fork: '',
  tree: '',
  settings: '',
  login: '',
  logout: '',
  trust: '',
  hotkeys: '',
  share: '',
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
    chat.notice('info', `/${name}: ${t(`slash.unsupported.${name}` as Key)}`);
    return true;
  }
  return false;
}
