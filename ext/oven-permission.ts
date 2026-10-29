/**
 * pi-oven permission gate.
 *
 * auto   - every tool runs without asking (pi's default behaviour).
 * manual - tools that can change things ask the web UI first; read-only tools still run.
 *
 * The web UI switches mode with `/oven-permission auto|manual`, and reads it back from the
 * "oven-permission" status. Questions use ctx.ui.confirm, which pi forwards to the browser.
 */
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";

type Mode = "auto" | "manual";
const READ_ONLY = new Set(["read", "grep", "find", "ls", "get_goal", "symbol_search", "project_report", "module_report", "read_symbol", "read_enclosing"]);

export default function (pi: ExtensionAPI) {
	let mode: Mode = process.env.PI_OVEN_PERMISSION === "manual" ? "manual" : "auto";

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
			if (next === "auto" || next === "manual") mode = next;
			ctx.ui.setStatus("oven-permission", mode);
		},
	});

	pi.on("session_start", async (_event, ctx) => {
		ctx.ui.setStatus("oven-permission", mode);
	});

	pi.on("tool_call", async (event, ctx) => {
		if (mode !== "manual" || READ_ONLY.has(event.toolName)) return undefined;
		if (!ctx.hasUI) return { block: true, reason: "Manual permission mode needs a UI to confirm" };
		const ok = await ctx.ui.confirm(`${event.toolName} 실행을 허용할까요?`, describe(event.toolName, event.input as Record<string, unknown>));
		return ok ? undefined : { block: true, reason: "The user declined this tool call" };
	});
}
