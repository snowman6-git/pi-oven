/**
 * pi-oven: a subscription's usage limits, as the provider reports them with each response.
 *
 * pi does not pass these on, so this reads them where they arrive and hands them to pi-oven as a status line
 * (`oven-limits`, JSON) that the server keeps per provider. Nothing is asked of the provider here: the numbers come with
 * replies the user's own messages caused. (The server also asks ChatGPT directly, see `src/usage/`.)
 *
 * One reader per provider, so another can be added without touching the rest:
 * - ChatGPT over WebSocket: a `codex.rate_limits` event at the start of each response (`chatgptFromEvent`).
 * - ChatGPT over HTTP (SSE): pi falls back to it when a WebSocket closes; then the same numbers come as
 *   `x-codex-primary-*` / `x-codex-secondary-*` response headers (`chatgptFromHeaders`).
 * - Claude (anthropic, subscription sign-in): `anthropic-ratelimit-unified-<window>-utilization` / `-reset` / `-status`
 *   headers, read as Claude Code reads them (`claudeFromHeaders`). They come only with a reply that got through: pi
 *   throws on an error status before this hook, so a 429 brings none (the server asks the usage address then).
 *   API-key use sends other headers and is left alone.
 */
import type { ExtensionAPI, ExtensionContext } from "@earendil-works/pi-coding-agent";
import { createHash } from "node:crypto";

type Window = { minutes: number; used: number; resetAt: number | null };
type Limits = { provider: string; plan: string | null; at: number; limitReached: boolean; windows: Window[] };
type Headers = Record<string, string>;

const KEY = "oven-limits";

function send(ctx: ExtensionContext, limits: Limits | null) {
	if (!limits || !limits.provider || !limits.windows.length) return;
	ctx.ui.setStatus(KEY, JSON.stringify(limits));
}

const num = (v: unknown): number | null => {
	const n = typeof v === "string" ? Number(v) : v;
	return typeof n === "number" && Number.isFinite(n) ? n : null;
};

/** A reset given as an absolute time (seconds) or as seconds from now. */
const resetFrom = (at: unknown, after: unknown, now: number): number | null => {
	const a = num(at);
	if (a != null) return a * 1000;
	const s = num(after);
	return s != null ? now + s * 1000 : null;
};

// ── ChatGPT ────────────────────────────────────────────────────────────────

function chatgptFromEvent(provider: string, d: any, now: number): Limits | null {
	if (d?.type !== "codex.rate_limits" || !d.rate_limits) return null;
	const r = d.rate_limits;
	const windows: Window[] = [];
	for (const w of [r.primary, r.secondary]) {
		const minutes = num(w?.window_minutes);
		const used = num(w?.used_percent);
		if (minutes == null || used == null) continue;
		windows.push({ minutes, used, resetAt: resetFrom(w?.reset_at, w?.reset_after_seconds, now) });
	}
	return {
		provider,
		plan: typeof d.plan_type === "string" ? d.plan_type : null,
		at: now,
		limitReached: r.limit_reached === true || r.allowed === false,
		windows,
	};
}

function chatgptFromHeaders(provider: string, h: Headers, status: number, now: number): Limits | null {
	const windows: Window[] = [];
	for (const slot of ["primary", "secondary"]) {
		const used = num(h[`x-codex-${slot}-used-percent`]);
		const minutes = num(h[`x-codex-${slot}-window-minutes`]);
		if (used == null || minutes == null) continue;
		windows.push({ minutes, used, resetAt: resetFrom(h[`x-codex-${slot}-reset-at`], h[`x-codex-${slot}-reset-after-seconds`], now) });
	}
	if (!windows.length) return null;
	return { provider, plan: h["x-codex-plan-type"] ?? null, at: now, limitReached: status === 429, windows };
}

// ── Claude ─────────────────────────────────────────────────────────────────

/** "5h" → 300, "7d" → 10080. Unknown names are skipped. */
function windowMinutes(name: string): number | null {
	const m = /^(\d{1,4})(m|h|d)$/.exec(name);
	if (!m) return null;
	return Number(m[1]) * (m[2] === "d" ? 1440 : m[2] === "h" ? 60 : 1);
}

function claudeFromHeaders(provider: string, h: Headers, status: number, now: number): Limits | null {
	// What Claude Code reads too: per window (`5h`, `7d`) `-utilization` as a fraction of the plan (0.42), `-reset` in
	// seconds since 1970 and `-status` (`allowed`, `allowed_warning`, `rejected`). The fraction can pass 1 at the
	// limit; it is never a percentage (reading 1.02 as 1.02 % showed a used-up plan as nearly unused).
	const windows: Window[] = [];
	let rejected = h["anthropic-ratelimit-unified-status"] === "rejected" || status === 429;
	for (const [k, v] of Object.entries(h)) {
		const m = /^anthropic-ratelimit-unified-([0-9a-z]{1,8})-utilization$/.exec(k);
		if (!m) continue;
		const minutes = windowMinutes(m[1]);
		const u = num(v);
		if (minutes == null || u == null) continue;
		const out = h[`anthropic-ratelimit-unified-${m[1]}-status`] === "rejected";
		rejected ||= out;
		windows.push({ minutes, used: out ? Math.max(100, u * 100) : u * 100, resetAt: resetFrom(h[`anthropic-ratelimit-unified-${m[1]}-reset`], null, now) });
	}
	if (!windows.length) return null;
	return { provider, plan: null, at: now, limitReached: rejected, windows };
}

// ── what each request looked like (for the server's log) ───────────────────

/**
 * The prompt cache only helps while each request starts with the request before it. When something changes an early
 * message (an extension rewriting the context, say), everything after it is written to the cache again, and a long
 * conversation costs many times what it should. So for every request this compares the messages with the request
 * before, by hash, and tells the server (status `oven-trace`) from which message on they differ. Nothing of the
 * content leaves pi: hashes stay here, only positions, roles and sizes go.
 */
const TRACE = "oven-trace";
let previous: { sys: string; tools: string; msgs: string[] } | null = null;

const hashOf = (v: unknown): string =>
	// `cache_control` marks move to the newest messages with every request: not a change of the content
	createHash("sha1").update(JSON.stringify(v, (k, x) => (k === "cache_control" ? undefined : x)) ?? "").digest("base64");

function trace(payload: any): Record<string, unknown> | null {
	if (!payload || typeof payload !== "object") return null;
	const msgs: unknown[] = Array.isArray(payload.messages) ? payload.messages : Array.isArray(payload.input) ? payload.input : Array.isArray(payload.contents) ? payload.contents : [];
	const now = { sys: hashOf(payload.system ?? payload.instructions ?? payload.systemInstruction ?? null), tools: hashOf(payload.tools ?? null), msgs: msgs.map(hashOf) };
	const marks: number[] = [];
	msgs.forEach((m, i) => {
		if (JSON.stringify(m).includes('"cache_control"')) marks.push(i);
	});
	const out: Record<string, unknown> = { at: Date.now(), n: msgs.length, marks, bytes: JSON.stringify(payload).length };
	if (previous) {
		const common = Math.min(previous.msgs.length, now.msgs.length);
		let first = 0;
		while (first < common && previous.msgs[first] === now.msgs[first]) first++;
		out.prevN = previous.msgs.length;
		// the first message that is not the one before at that place (`null`: this request only adds to the last one)
		out.first = first < common ? first : null;
		if (first < common) out.role = (msgs[first] as any)?.role ?? (msgs[first] as any)?.type ?? null;
		out.sys = now.sys !== previous.sys;
		out.tools = now.tools !== previous.tools;
	}
	previous = now;
	return out;
}

// ── wiring ─────────────────────────────────────────────────────────────────

export default function (pi: ExtensionAPI) {
	pi.on("before_provider_request", (event, ctx) => {
		try {
			const t = trace(event.payload);
			if (t) ctx.ui.setStatus(TRACE, JSON.stringify(t));
		} catch {
			// a log line is not worth a failed request
		}
		return undefined;
	});

	pi.on("provider_stream_event", (event, ctx) => {
		send(ctx, chatgptFromEvent(String(event.provider), event.data, Date.now()));
	});

	pi.on("after_provider_response", (event, ctx) => {
		const h: Headers = {};
		for (const [k, v] of Object.entries(event.headers ?? {})) h[k.toLowerCase()] = String(v);
		const now = Date.now();
		send(
			ctx,
			chatgptFromHeaders(ctx.model?.provider ?? "openai-codex", h, event.status, now) ??
				claudeFromHeaders(ctx.model?.provider ?? "anthropic", h, event.status, now),
		);
	});
}
