/**
 * pi-oven "100h mode": the part that has to run inside pi.
 *
 * After pi has used up its own retries the failed request is over, and RPC has no command that repeats it. The server
 * keeps the schedule (it sees pi's events, whether or not a browser is open) and, when a try is due, runs `/oven-retry`.
 * This sends a hidden message that starts the turn again, so no bubble appears in the chat.
 *
 * Only the newest of those messages goes to the model: a hundred tries must not leave a hundred lines in the context.
 *
 * `/oven-note <json>` keeps a note of what the server did for the person (Baton Touch handed the session to another
 * account or model, or that it compacted or acted on a nearly used plan) in the session, so the page still shows it after a reload. Notes never go to the model.
 *
 * `/oven-models` reads models.json again (pi reads it only when it starts, and RPC has no command for it), after the
 * page changed a model there. The model in use is put back on as it now reads (a new context window, say), keeping
 * its reasoning level.
 */
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";

const TYPE = "oven-retry";
const TEXT = "The previous request failed with a temporary API error. Continue from where you left off.";
/** The command the server runs (`/oven-note <json>`), and the kind of message it keeps. */
const NOTE_COMMAND = "oven-note";
const NOTE = "oven-baton";
/** What the server did while nobody was working (compaction while resting, the action for a plan nearly used): `{"note":"rest",…}`. */
const REST = "oven-rest";
const MODELS_COMMAND = "oven-models";

export default function (pi: ExtensionAPI) {
	pi.registerCommand(TYPE, {
		description: "Try the failed request again (used by pi-oven)",
		handler: async (_args, ctx) => {
			if (!ctx.isIdle()) return;
			pi.sendMessage({ customType: TYPE, content: TEXT, display: false }, { triggerTurn: true });
		},
	});

	pi.registerCommand(NOTE_COMMAND, {
		description: "Keep a note of an account or model hand-over (used by pi-oven)",
		handler: async (args, ctx) => {
			let details: unknown;
			try {
				details = JSON.parse(args);
			} catch {
				return;
			}
			if (typeof details !== "object" || details === null || Array.isArray(details)) return;
			const rest = (details as { note?: unknown }).note === "rest";
			const message = { customType: rest ? REST : NOTE, content: "", display: true, details };
			if (ctx.isIdle()) pi.sendMessage(message);
			else pi.sendMessage(message, { deliverAs: "nextTurn" });
		},
	});

	pi.registerCommand(MODELS_COMMAND, {
		description: "Read models.json again (used by pi-oven)",
		handler: async (_args, ctx) => {
			try {
				await ctx.modelRegistry.refresh();
			} catch {
				return;
			}
			const cur = ctx.model;
			const next = cur && ctx.modelRegistry.find(cur.provider, cur.id);
			if (!cur || !next || JSON.stringify(next) === JSON.stringify(cur)) return;
			const level = pi.getThinkingLevel();
			try {
				if (await pi.setModel(next)) pi.setThinkingLevel(level);
			} catch {
				// no key for it any more: the session stays on the model as it was
			}
		},
	});

	pi.on("context", async (event) => {
		const isOurs = (m: { role: string; customType?: string }) => m.role === "custom" && m.customType === TYPE;
		const isNote = (m: { role: string; customType?: string }) => m.role === "custom" && (m.customType === NOTE || m.customType === REST);
		// Collapse consecutive retry messages (e.g. 100 failed tries in a row → 1) without retroactively deleting
		// retry messages from earlier completed turns, which would invalidate the prompt cache.
		const kept: typeof event.messages = [];
		let changed = false;
		for (const m of event.messages) {
			if (isNote(m)) {
				changed = true;
				continue;
			}
			if (isOurs(m) && kept.length > 0 && isOurs(kept[kept.length - 1])) {
				kept[kept.length - 1] = m;
				changed = true;
				continue;
			}
			kept.push(m);
		}
		return changed ? { messages: kept } : undefined;
	});
}
