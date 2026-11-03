/**
 * pi-oven permission gate.
 *
 * auto   - every tool runs without asking (pi's default behaviour).
 * manual - tools that can change things ask the web UI first; read-only tools still run.
 *
 * The web UI switches mode with `/oven-permission auto|manual`, and reads it back from the
 * "oven-permission" status. Questions use ctx.ui.confirm, which pi forwards to the browser.
 *
 * The mode is kept in a file the server names (PI_OVEN_PERMISSION_FILE), shared by every session and device:
 * a pi that starts with no browser attached is already in the chosen mode. "auto-default" means nothing was
 * ever chosen (the web UI may then bring over the choice it kept from before).
 */
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { readFileSync, renameSync, writeFileSync } from "node:fs";

type Mode = "auto" | "manual";
const READ_ONLY = new Set(["read", "grep", "find", "ls", "get_goal", "symbol_search", "project_report", "module_report", "read_symbol", "read_enclosing"]);

export default function (pi: ExtensionAPI) {
	const file = process.env.PI_OVEN_PERMISSION_FILE;
	const saved = (): Mode | null => {
		try {
			const v = file ? readFileSync(file, "utf8").trim() : "";
			return v === "manual" || v === "auto" ? v : null;
		} catch {
			return null;
		}
	};
	let chosen = saved() !== null;
	let mode: Mode = saved() ?? (process.env.PI_OVEN_PERMISSION === "manual" ? "manual" : "auto");
	const status = () => (chosen ? mode : `${mode}-default`);

	const describe = (name: string, input: Record<string, unknown>): string => {
		if (name === "bash") return String(input.command ?? "");
		if (typeof input.path === "string") return `${name} ${input.path}`;
		const first = Object.values(input).find((v) => typeof v === "string") as string | undefined;
		return first ? `${name}: ${first.slice(0, 300)}` : name;
	};

	pi.registerCommand("oven-permission", {
		description: "Set tool permission: auto or manual",
		handler: async (arg, ctx) => {
			const next = String(arg ?? "").trim();
			if (next === "auto" || next === "manual") {
				mode = next;
				chosen = true;
				if (file) {
					try {
						writeFileSync(file + ".tmp", mode);
						renameSync(file + ".tmp", file);
					} catch {
						/* this process still follows it */
					}
				}
			}
			ctx.ui.setStatus("oven-permission", status());
		},
	});

	pi.on("session_start", async (_event, ctx) => {
		ctx.ui.setStatus("oven-permission", status());
	});

	pi.on("tool_call", async (event, ctx) => {
		const now = saved(); // another session or device may have changed it since
		if (now && now !== mode) {
			mode = now;
			chosen = true;
			ctx.ui.setStatus("oven-permission", status());
		}
		if (mode !== "manual" || READ_ONLY.has(event.toolName)) return undefined;
		if (!ctx.hasUI) return { block: true, reason: "Manual permission mode needs a UI to confirm" };
		// pi-oven's page words this title in its own language (it knows this exact shape)
		const ok = await ctx.ui.confirm(`Allow ${event.toolName} to run?`, describe(event.toolName, event.input as Record<string, unknown>));
		return ok ? undefined : { block: true, reason: "The user declined this tool call" };
	});
}
