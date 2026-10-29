// What the conversation is made of, and how pi's messages become it. Pure: no state, no network.

import type { ImagePart, Usage, WireMessage } from './protocol';
import type { Speed } from './speed';
import { textOf } from './util.ts';

export interface ToolBlock {
  type: 'tool';
  id: string;
  name: string;
  args: Record<string, any>;
  status: 'pending' | 'running' | 'ok' | 'error';
  output: string;
  details?: any;
  images?: ImagePart[];
  durationMs?: number;
  /** When this page saw the call start (for the running clock). The final time comes from pi (`durationMs`). */
  startedAt?: number;
}
export type Block = { type: 'text'; text: string } | { type: 'thinking'; text: string; redacted?: boolean } | ToolBlock;

export type Item =
  | { kind: 'user'; key: number; text: string; images: ImagePart[]; skills: string[]; ts?: number }
  | { kind: 'assistant'; key: number; blocks: Block[]; live: boolean; ts?: number; model?: string; usage?: Usage; stop?: string; error?: string; speed?: Speed }
  | { kind: 'bash'; key: number; command: string; output: string; exitCode?: number; cancelled: boolean; truncated: boolean; running: boolean; ts?: number }
  | { kind: 'summary'; key: number; label: string; text: string; ts?: number }
  /** A background task ended (pi's own notification, shown as a bubble that says so). */
  | { kind: 'task'; key: number; id: string; name: string; command: string; status: string; exitCode?: number | null; durationMs?: number; text: string; ts?: number }
  | { kind: 'notice'; key: number; level: 'info' | 'warn' | 'error'; text: string; ts?: number };

const SKILL_BLOCK = /<skill name="([^"]+)"[^>]*>[\s\S]*?<\/skill>\s*/g;

// Extensions name their injected messages for machines; show something a person can read.
const CUSTOM_LABELS: Record<string, string> = { 'background-task-notification': '백그라운드 작업 알림' };
export const customLabel = (t?: string) => (t && CUSTOM_LABELS[t]) || t || '확장';

let keySeq = 0;
export const nextKey = () => ++keySeq;

export function blocksFromContent(content: WireMessage['content'], known: Map<string, ToolBlock>): Block[] {
  if (!Array.isArray(content)) return typeof content === 'string' && content ? [{ type: 'text', text: content }] : [];
  const out: Block[] = [];
  for (const p of content as any[]) {
    if (p.type === 'text') out.push({ type: 'text', text: p.text });
    else if (p.type === 'thinking') out.push({ type: 'thinking', text: p.thinking ?? '', redacted: p.redacted });
    else if (p.type === 'toolCall') {
      const prev = known.get(p.id);
      out.push({
        type: 'tool', id: p.id, name: p.name, args: p.arguments ?? {},
        status: prev?.status ?? 'pending', output: prev?.output ?? '', details: prev?.details,
        images: prev?.images, durationMs: prev?.durationMs,
      });
    }
  }
  return out;
}

export function userItem(m: WireMessage): Item {
  const raw = textOf(m.content);
  const skills: string[] = [];
  const text = raw.replace(SKILL_BLOCK, (_, name) => (skills.push(name), '')).trim();
  const images = Array.isArray(m.content) ? (m.content.filter((p: any) => p.type === 'image') as ImagePart[]) : [];
  return { kind: 'user', key: nextKey(), text, images, skills, ts: m.timestamp };
}

/** Build display items from a full message history (initial load, resync). */
export function itemsFromMessages(msgs: WireMessage[]): Item[] {
  const items: Item[] = [];
  const tools = new Map<string, ToolBlock>();
  for (const m of msgs) {
    switch (m.role) {
      case 'user': items.push(userItem(m)); break;
      case 'assistant': {
        const blocks = blocksFromContent(m.content, tools);
        for (const b of blocks) if (b.type === 'tool') tools.set(b.id, b);
        items.push({
          kind: 'assistant', key: nextKey(), blocks, live: m.stopReason === 'pending', ts: m.timestamp,
          model: m.model, usage: m.usage, stop: m.stopReason, error: m.errorMessage,
        });
        break;
      }
      case 'toolResult': {
        const t = m.toolCallId ? tools.get(m.toolCallId) : undefined;
        if (t) applyResult(t, m);
        break;
      }
      case 'bashExecution':
        items.push({
          kind: 'bash', key: nextKey(), command: m.command ?? '', output: m.output ?? '', exitCode: m.exitCode,
          cancelled: !!m.cancelled, truncated: !!m.truncated, running: false, ts: m.timestamp,
        });
        break;
      case 'compactionSummary':
        items.push({ kind: 'summary', key: nextKey(), label: '컨텍스트 압축됨', text: m.summary ?? '', ts: m.timestamp });
        break;
      case 'branchSummary':
        items.push({ kind: 'summary', key: nextKey(), label: '분기 요약', text: m.summary ?? '', ts: m.timestamp });
        break;
      case 'custom': {
        const t = taskItem(m);
        if (t) items.push(t);
        else if (m.display) items.push({ kind: 'summary', key: nextKey(), label: customLabel(m.customType), text: textOf(m.content), ts: m.timestamp });
        break;
      }
    }
  }
  return items;
}

/** pi tells the conversation when a background task ends; the machine-readable text is not for people, the details are. */
export function taskItem(m: WireMessage): Item | null {
  const d = m.details;
  if (m.customType !== 'background-task-notification' || !m.display || !d?.id) return null;
  const durationMs = typeof d.startTime === 'number' && typeof d.endTime === 'number' ? d.endTime - d.startTime : undefined;
  return { kind: 'task', key: nextKey(), id: String(d.id), name: String(d.name ?? d.command ?? d.id), command: String(d.command ?? ''), status: String(d.status ?? ''), exitCode: d.exitCode, durationMs, text: textOf(m.content), ts: m.timestamp };
}

export function applyResult(t: ToolBlock, m: { content?: WireMessage['content']; isError?: boolean; details?: any }) {
  t.output = textOf(m.content);
  t.images = Array.isArray(m.content) ? (m.content.filter((p: any) => p.type === 'image') as ImagePart[]) : undefined;
  t.details = m.details;
  t.status = m.isError ? 'error' : 'ok';
}
