/**
 * pi-oven: a subscription's usage limits, as the provider reports them with each response.
 *
 * pi does not pass these on, so this reads them where they arrive and hands them to pi-oven as a status line
 * (`oven-limits`, JSON) that the server keeps per provider. Nothing is asked of the provider: the numbers come with
 * replies the user's own messages caused, so they are as fresh as the last reply.
 *
 * - ChatGPT (openai-codex): a `codex.rate_limits` event at the start of each response (WebSocket and SSE alike).
 * - Claude (anthropic, subscription sign-in): `anthropic-ratelimit-unified-<window>-utilization` / `-reset` headers.
 *   API-key use sends other headers and is left alone.
 */
import type { ExtensionAPI, ExtensionContext } from "@earendil-works/pi-coding-agent";

type Window = { minutes: number; used: number; resetAt: number | null };
type Limits = { provider: string; plan: string | null; at: number; limitReached: boolean; windows: Window[] };

const KEY = "oven-limits";

function send(ctx: ExtensionContext, limits: Limits) {
	if (!limits.provider || !limits.windows.length) return;
	ctx.ui.setStatus(KEY, JSON.stringify(limits));
}

const num = (v: unknown): number | null => {
	const n = typeof v === "string" ? Number(v) : v;
	return typeof n === "number" && Number.isFinite(n) ? n : null;
};

function codexWindow(w: any, now: number): Window | null {
	const minutes = num(w?.window_minutes);
	const used = num(w?.used_percent);
	if (minutes == null || used == null) return null;
	const at = num(w?.reset_at);
	const after = num(w?.reset_after_seconds);
	const resetAt = at != null ? at * 1000 : after != null ? now + after * 1000 : null;
	return { minutes, used, resetAt };
}

/** "5h" → 300, "7d" → 10080. Unknown names are skipped. */
function windowMinutes(name: string): number | null {
	const m = /^(\d{1,4})(m|h|d)$/.exec(name);
	if (!m) return null;
	return Number(m[1]) * (m[2] === "d" ? 1440 : m[2] === "h" ? 60 : 1);
}

export default function (pi: ExtensionAPI) {
	pi.on("provider_stream_event", (event, ctx) => {
		const d = event.data as any;
		if (d?.type !== "codex.rate_limits" || !d.rate_limits) return;
		const now = Date.now();
		const r = d.rate_limits;
		const windows = [r.primary, r.secondary].map((w) => codexWindow(w, now)).filter((w): w is Window => !!w);
		send(ctx, {
			provider: String(event.provider),
			plan: typeof d.plan_type === "string" ? d.plan_type : null,
			at: now,
			limitReached: r.limit_reached === true || r.allowed === false,
			windows,
		});
	});

	pi.on("after_provider_response", (event, ctx) => {
		const h = event.headers ?? {};
		const now = Date.now();
		const windows: Window[] = [];
		for (const [k, v] of Object.entries(h)) {
			const m = /^anthropic-ratelimit-unified-([0-9a-z]{1,8})-utilization$/.exec(k.toLowerCase());
			if (!m) continue;
			const minutes = windowMinutes(m[1]);
			const u = num(v);
			if (minutes == null || u == null) continue;
			const reset = num(h[`anthropic-ratelimit-unified-${m[1]}-reset`]);
			// a fraction (0.42), but take a percentage too
			windows.push({ minutes, used: u <= 1 ? u * 100 : u, resetAt: reset != null ? reset * 1000 : null });
		}
		if (!windows.length) return;
		const status = String(h["anthropic-ratelimit-unified-status"] ?? "");
		send(ctx, {
			provider: ctx.model?.provider ?? "anthropic",
			plan: null,
			at: now,
			limitReached: status === "rejected" || event.status === 429,
			windows,
		});
	});
}
