// Shapes of the records pi emits in `--mode rpc`, reduced to what the UI reads.
// Unknown fields are tolerated; pi's own docs are the source of truth.

export type ThinkingLevel = 'off' | 'minimal' | 'low' | 'medium' | 'high' | 'xhigh' | 'max';

export interface TextPart { type: 'text'; text: string }
export interface ImagePart { type: 'image'; data: string; mimeType: string }
export interface ThinkingPart { type: 'thinking'; thinking: string; redacted?: boolean }
export interface ToolCallPart { type: 'toolCall'; id: string; name: string; arguments: Record<string, any> }

export interface Usage {
  input: number; output: number; cacheRead: number; cacheWrite: number; totalTokens: number;
  cost: { total: number };
}

export interface WireMessage {
  role: string;
  content?: string | Array<TextPart | ImagePart | ThinkingPart | ToolCallPart>;
  timestamp?: number;
  // assistant
  model?: string; provider?: string; usage?: Usage; stopReason?: string; errorMessage?: string;
  // toolResult
  toolCallId?: string; toolName?: string; isError?: boolean; details?: any;
  // bashExecution
  command?: string; output?: string; exitCode?: number; cancelled?: boolean; truncated?: boolean;
  // summaries / custom
  summary?: string; tokensBefore?: number; customType?: string; display?: boolean;
}

export interface ModelInfo {
  id: string; name?: string; provider: string; reasoning?: boolean; contextWindow?: number;
}

export interface SessionState {
  model?: ModelInfo;
  thinkingLevel: ThinkingLevel;
  isStreaming: boolean;
  isCompacting: boolean;
  sessionFile?: string;
  sessionId: string;
  sessionName?: string;
  autoCompactionEnabled: boolean;
  messageCount: number;
  pendingMessageCount: number;
}

export interface SessionStats {
  userMessages: number; assistantMessages: number; toolCalls: number;
  tokens: { input: number; output: number; cacheRead: number; cacheWrite: number; total: number };
  cost: number;
  contextUsage?: { tokens: number | null; contextWindow: number; percent: number | null };
}

export interface SlashCommand { name: string; description?: string; source: 'extension' | 'prompt' | 'skill' }

export interface Response {
  type: 'response'; id?: string; command: string; success: boolean; data?: any; error?: string;
}

export type ExtensionRequest =
  | { type: 'extension_ui_request'; id: string; method: 'select'; title: string; options: string[]; timeout?: number }
  | { type: 'extension_ui_request'; id: string; method: 'confirm'; title: string; message: string; timeout?: number }
  | { type: 'extension_ui_request'; id: string; method: 'input'; title: string; placeholder?: string; timeout?: number }
  | { type: 'extension_ui_request'; id: string; method: 'editor'; title: string; prefill?: string }
  | { type: 'extension_ui_request'; id: string; method: 'notify'; message: string; notifyType?: 'info' | 'warning' | 'error' }
  | { type: 'extension_ui_request'; id: string; method: 'setStatus'; statusKey: string; statusText?: string }
  | { type: 'extension_ui_request'; id: string; method: 'setWidget'; widgetKey: string; widgetLines?: string[]; widgetPlacement?: string }
  | { type: 'extension_ui_request'; id: string; method: 'setTitle'; title: string }
  | { type: 'extension_ui_request'; id: string; method: 'set_editor_text'; text: string };

export interface BgTask {
  id: string; name?: string; command: string; description?: string;
  status: 'running' | 'completed' | 'failed' | 'killed';
  startTime: number; endTime?: number; exitCode?: number | null; bytesWritten?: number;
  isAgent?: boolean; error?: string; model?: string;
}

export interface SessionSummary {
  path: string; id: string; cwd: string; name: string | null; preview: string | null; modified: number; turns: number;
}
export interface RuntimeInfo { id: string; cwd: string; sessionPath: string | null; pid: number | null; startedAt: number }
export interface DirListing { path: string; parent: string | null; entries: { name: string; path: string }[]; isGit: boolean }
