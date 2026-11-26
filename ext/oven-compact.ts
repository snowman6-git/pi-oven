/**
 * pi-oven: the model that writes a compaction's summary (Settings → Auto → Compaction → model).
 *
 * pi summarizes with the conversation's own model. When a model is chosen in Settings (`model` in pi-oven's
 * `auto-compact.json`, read again at every compaction so an open session follows a change at once), the summary is
 * written by that one instead: what pi would summarize (the messages before the cut, the start of a split turn, and
 * the previous summary), with the same kept-from point. Anything that goes wrong leaves it to pi's own compaction.
 * This covers every compaction: pi's own, `/compact`, and the ones pi-oven starts.
 */
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { convertToLlm, serializeConversation } from "@earendil-works/pi-coding-agent";
import { uuidv7 } from "@earendil-works/pi-ai";
import { readFileSync } from "node:fs";

const FILE_VAR = "PI_OVEN_AUTO_COMPACT_FILE";
const MAX_OUTPUT = 8192;

/** "provider/modelId" from the settings file, or null (the conversation's own model). */
function chosenModel(): string | null {
	const file = process.env[FILE_VAR];
	if (!file) return null;
	try {
		const m = JSON.parse(readFileSync(file, "utf8")).model;
		return typeof m === "string" && m.includes("/") ? m : null;
	} catch {
		return null;
	}
}

const INSTRUCTIONS = `Summarize the conversation below so work can continue from the summary alone. Use these sections:
## Goal
## Decisions (and why)
## Files (full paths, what was changed or is still relevant)
## Current state
## Remaining work and next step
Keep the user's standing instructions and preferences. Drop tool output and dead ends. Be concise.`;

/** The files read and changed, as pi appends them to a summary (its own helpers are not exported). */
function fileLists(ops: { read: Set<string>; written: Set<string>; edited: Set<string> }) {
	const modified = new Set([...ops.edited, ...ops.written]);
	return { readFiles: [...ops.read].filter((f) => !modified.has(f)).sort(), modifiedFiles: [...modified].sort() };
}

function fileSections(readFiles: string[], modifiedFiles: string[]): string {
	const sections: string[] = [];
	if (readFiles.length) sections.push(`<read-files>\n${readFiles.join("\n")}\n</read-files>`);
	if (modifiedFiles.length) sections.push(`<modified-files>\n${modifiedFiles.join("\n")}\n</modified-files>`);
	return sections.length ? `\n\n${sections.join("\n\n")}` : "";
}

export default function (pi: ExtensionAPI) {
	pi.on("session_before_compact", async (event, ctx) => {
		const want = chosenModel();
		if (!want) return;
		const { preparation, signal, customInstructions } = event;
		const back = (why: string) => {
			if (!signal.aborted) ctx.ui.notify(`Compaction model ${want}: ${why}; using the default compaction`, "warning");
			return undefined;
		};
		const at = want.indexOf("/");
		const model = ctx.modelRegistry.find(want.slice(0, at), want.slice(at + 1));
		if (!model) return back("not found");

		const { messagesToSummarize, turnPrefixMessages, previousSummary, firstKeptEntryId, tokensBefore, fileOps } = preparation;
		// the context being summarized must fit with room for the answer (a token count, not characters: no guess by language)
		if (tokensBefore + MAX_OUTPUT > model.contextWindow) return back("its context window is too small");

		const text = serializeConversation(convertToLlm([...messagesToSummarize, ...turnPrefixMessages]));
		const previous = previousSummary ? `Previous summary:\n${previousSummary}\n\n` : "";
		const focus = customInstructions?.trim() ? `\n\nAdditional focus: ${customInstructions.trim()}` : "";
		const prompt = `${INSTRUCTIONS}${focus}\n\n${previous}<conversation>\n${text}\n</conversation>`;

		try {
			const response = await ctx.modelRegistry.complete(
				model,
				{ messages: [{ role: "user" as const, content: [{ type: "text" as const, text: prompt }], timestamp: Date.now() }] },
				{ maxTokens: MAX_OUTPUT, signal, cacheRetention: "none", sessionId: uuidv7() },
			);
			if (signal.aborted) return;
			// pi's own rule: an error or a cut-off answer is never kept as a checkpoint
			if (response.stopReason === "error") return back(response.errorMessage || "the call failed");
			if (response.stopReason === "length") return back("the summary was cut off");
			const body = response.content
				.filter((c): c is { type: "text"; text: string } => c.type === "text")
				.map((c) => c.text)
				.join("\n");
			if (!body.trim()) return back("empty summary");
			const { readFiles, modifiedFiles } = fileLists(fileOps);
			const summary = body + fileSections(readFiles, modifiedFiles);
			return { compaction: { summary, firstKeptEntryId, tokensBefore, usage: response.usage, details: { readFiles, modifiedFiles } } };
		} catch (error) {
			return back(error instanceof Error ? error.message : String(error));
		}
	});
}
