/**
 * `delegate` — the custom tool that lets the editor hand a task off to a
 * specialist role in-process.
 *
 * Phase 3 implementation:
 *   - target is restricted to the 4 specialist roles (no editor → editor
 *     recursion).
 *   - up to 2 concurrent delegations across the whole server (queue the
 *     rest). Per the open question in docs/PI_MIGRATION_PLAN.md this will
 *     become per-editor-session in Phase 4.
 *   - the delegated run is a normal `runRole(..., { source: "delegate" })`,
 *     so it shows up in `/runs`, in `/status` live tails, and writes the
 *     same .runtime/runs/<id>.{jsonl,json} artifacts.
 *   - the tool's result text is the specialist's final assistant message.
 */
import { Type } from "typebox";
import { defineTool, type ToolDefinition } from "@mariozechner/pi-coding-agent";
import { getRole } from "../roles/registry.js";
import type { RoleToolBuilderDeps, RunSource } from "../roles/types.js";
import { runRole } from "./runRole.js";

const MAX_CONCURRENT_DELEGATIONS = 2;

class Semaphore {
	private active = 0;
	private waiters: Array<() => void> = [];
	constructor(private max: number) {}
	async acquire(): Promise<void> {
		if (this.active < this.max) {
			this.active++;
			return;
		}
		await new Promise<void>((res) => this.waiters.push(res));
		this.active++;
	}
	release(): void {
		this.active--;
		const next = this.waiters.shift();
		if (next) next();
	}
}

const globalDelegateSemaphore = new Semaphore(MAX_CONCURRENT_DELEGATIONS);

const DelegateParams = Type.Object({
	target: Type.Union(
		[
			Type.Literal("investigator"),
			Type.Literal("archivist"),
			Type.Literal("data-reporter"),
			Type.Literal("watch-runner"),
		],
		{ description: "Specialist role to delegate to." },
	),
	prompt: Type.String({
		description: "Task description for the specialist — what to investigate, ingest, or query.",
	}),
	context: Type.Optional(
		Type.String({
			description: "Optional extra context (excerpts, page references) to inject into the specialist's prompt.",
		}),
	),
});

type DelegateDetails = Record<string, unknown>;

export function buildDelegateTool(deps: RoleToolBuilderDeps): ToolDefinition {
	return defineTool<typeof DelegateParams, DelegateDetails>({
		name: "delegate",
		label: "Delegate to specialist",
		description:
			"Hand a task to a specialist role in-process. Use for in-depth analysis that benefits from a specialist's " +
			"skill prompt. The specialist runs as a separate agent session whose events appear in /status. Returns " +
			"the specialist's final answer as text. Do NOT delegate trivial questions you can answer from existing " +
			"wiki/DB material yourself.",
		promptSnippet:
			"delegate(target, prompt) — hand off to investigator / archivist / data-reporter / watch-runner.",
		parameters: DelegateParams,
		execute: async (_toolCallId, params) => {
			const role = getRole(deps.config, params.target);
			if (!role) {
				return {
					content: [{ type: "text", text: `[delegate] unknown target role: ${params.target}` }],
					details: { target: params.target, status: "unknown_role" },
					isError: true,
				};
			}

			const combinedPrompt = params.context
				? `${params.prompt}\n\n---\nContext from editor:\n${params.context}`
				: params.prompt;

			await globalDelegateSemaphore.acquire();
			const startedAt = Date.now();
			try {
				const result = await runRole(deps, role, {
					prompt: combinedPrompt,
					source: "delegate" satisfies RunSource,
				});
				const summary =
					(result.finalText && result.finalText.trim()) ||
					`[delegate] ${params.target} returned no final text (run ${result.runId}, ok=${result.ok})`;
				return {
					content: [
						{
							type: "text",
							text:
								`[delegate → ${params.target}] runId=${result.runId} ok=${result.ok} ` +
								`duration=${result.durationMs}ms\n\n${summary}`,
						},
					],
					details: {
						target: params.target,
						runId: result.runId,
						ok: result.ok,
						durationMs: result.durationMs,
						elapsedMs: Date.now() - startedAt,
						toolCalls: result.toolCalls,
					},
					isError: !result.ok,
				};
			} catch (err) {
				const msg = err instanceof Error ? err.message : String(err);
				return {
					content: [{ type: "text", text: `[delegate] error: ${msg}` }],
					details: { target: params.target, status: "error", error: msg },
					isError: true,
				};
			} finally {
				globalDelegateSemaphore.release();
			}
		},
	});
}
