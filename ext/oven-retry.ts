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
 * account or model) in the session, so the page still shows it after a reload. Notes never go to the model.
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
const MODELS_COMMAND = "oven-models";

export default function (pi: ExtensionAPI) {
	pi.registerCommand(TYPE, {
		description: "Try the failed request again (used by pi-oven)",
		handler: async () => {
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
			const message = { customType: NOTE, content: "", display: true, details };
			if (ctx.isIdle()) pi.sendMessage(message);
			else pi.sendMessage(message, { deliverAs: "nextTurn" });
		},
	});

	pi.registerCommand(MODELS_COMMAND, {
		description: "Read models.json again (used by pi-oven)",
		handler: async (_args, ctx) => {
			await ctx.modelRegistry.refresh();
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
		const isNote = (m: { role: string; customType?: string }) => m.role === "custom" && m.customType === NOTE;
		const last = event.messages.findLastIndex(isOurs);
		if (last < 0 && !event.messages.some(isNote)) return undefined;
		return { messages: event.messages.filter((m, i) => !isNote(m) && (i === last || !isOurs(m))) };
	});
}
