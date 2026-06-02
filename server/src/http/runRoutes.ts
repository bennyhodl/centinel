/**
 * Run-related HTTP routes.
 *
 *   POST /run/:role           — start a role run; { runId } on success.
 *                               If Accept: text/event-stream, the response
 *                               body is the live SSE event stream and
 *                               closes when the run completes.
 *   GET  /runs                — list recent runs (newest first).
 *   GET  /runs/:id            — return summary JSON for one run.
 *   GET  /runs/:id/events     — SSE: replay any existing events, then tail
 *                               live ones if the run is still active.
 */
import { randomUUID } from "node:crypto";
import type { IncomingMessage, ServerResponse } from "node:http";
import type { ServerConfig } from "../config.js";
import { getRole, listRoleNames } from "../roles/registry.js";
import type { RunInput, RunSource } from "../roles/types.js";
import { runRole } from "../runtime/runRole.js";
import type { RunStore } from "../runtime/runStore.js";
import { HttpError, errorJson, json, readJsonBody, startSse, wantsSse } from "./util.js";

export interface RunRoutesDeps {
	config: ServerConfig;
	store: RunStore;
}

interface RunRequestBody {
	prompt?: unknown;
	source?: unknown;
	runId?: unknown;
	context?: unknown;
}

function parseRunInput(body: RunRequestBody, defaultSource: RunSource): RunInput {
	if (typeof body.prompt !== "string" || body.prompt.trim() === "") {
		throw new HttpError(400, "invalid_prompt", "`prompt` must be a non-empty string");
	}
	const allowedSources: RunSource[] = ["cron", "http", "delegate", "cli"];
	const source =
		typeof body.source === "string" && (allowedSources as string[]).includes(body.source)
			? (body.source as RunSource)
			: defaultSource;
	const input: RunInput = { prompt: body.prompt, source };
	if (typeof body.runId === "string" && body.runId.trim() !== "") input.runId = body.runId;
	if (body.context && typeof body.context === "object") {
		input.context = body.context as Record<string, unknown>;
	}
	return input;
}

export async function handlePostRun(
	deps: RunRoutesDeps,
	roleName: string,
	req: IncomingMessage,
	res: ServerResponse,
): Promise<void> {
	const role = getRole(deps.config, roleName);
	if (!role) {
		json(res, 404, {
			ok: false,
			error: "unknown_role",
			role: roleName,
			known: listRoleNames(),
		});
		return;
	}

	let body: RunRequestBody;
	try {
		body = await readJsonBody<RunRequestBody>(req);
	} catch (err) {
		errorJson(res, err);
		return;
	}

	let input: RunInput;
	try {
		input = parseRunInput(body, "http");
	} catch (err) {
		errorJson(res, err);
		return;
	}

	if (wantsSse(req)) {
		// Stream mode: pre-generate the runId, announce it immediately, then
		// attach a subscriber once runRole registers the active run.
		const runId = input.runId ?? randomUUID();
		input.runId = runId;
		const sse = startSse(req, res);
		sse.send({ type: "_run_announced", runId, role: role.name });

		const runPromise = runRole(deps, role, input).catch((err: unknown) => {
			const msg = err instanceof Error ? err.message : String(err);
			sse.send({ type: "_runtime_error", error: msg });
			return undefined;
		});

		// Wait (briefly) for runRole to register the active run, then
		// subscribe. registerActive() is the first thing runRole does after
		// the logger opens, so this resolves in <10ms in practice.
		let attached = false;
		const attach = setInterval(() => {
			const active = deps.store.getActive(runId);
			if (!active) return;
			clearInterval(attach);
			attached = true;
			for (const ev of active.buffered) sse.send(ev);
			const unsub = deps.store.subscribe(runId, (ev) => sse.send(ev));
			sse.onClose(unsub);
		}, 10);

		await runPromise;
		clearInterval(attach);
		if (!attached) {
			// runRole finished before we attached — replay from disk.
			for await (const ev of deps.store.replayFromDisk(runId)) sse.send(ev);
		}
		sse.send({ type: "_run_end", runId });
		sse.close();
		return;
	}

	// Synchronous JSON mode.
	try {
		const result = await runRole(deps, role, input);
		json(res, result.ok ? 200 : 500, result);
	} catch (err) {
		errorJson(res, err);
	}
}

export function handleListRuns(deps: RunRoutesDeps, _req: IncomingMessage, res: ServerResponse): void {
	const runs = deps.store.listRecent(100);
	json(res, 200, { ok: true, runs });
}

export function handleGetRun(deps: RunRoutesDeps, runId: string, _req: IncomingMessage, res: ServerResponse): void {
	const summary = deps.store.getSummaryFromDisk(runId);
	if (summary) {
		json(res, 200, summary);
		return;
	}
	const active = deps.store.getActive(runId);
	if (active) {
		json(res, 200, {
			runId: active.runId,
			role: active.role,
			source: active.source,
			startedAt: active.startedAt,
			status: "in_progress",
		});
		return;
	}
	json(res, 404, { ok: false, error: "unknown_run", runId });
}

export async function handleRunEvents(
	deps: RunRoutesDeps,
	runId: string,
	req: IncomingMessage,
	res: ServerResponse,
): Promise<void> {
	const active = deps.store.getActive(runId);
	const sse = startSse(req, res);

	if (active) {
		// Replay buffered events first to avoid races.
		for (const ev of active.buffered) sse.send(ev);
		const unsub = deps.store.subscribe(runId, (ev) => {
			sse.send(ev);
			if (ev.event.type === "_run_end") {
				unsub();
				sse.close();
			}
		});
		sse.onClose(unsub);
		return;
	}

	// Inactive: replay everything from disk and close.
	let any = false;
	for await (const ev of deps.store.replayFromDisk(runId)) {
		any = true;
		sse.send(ev);
	}
	if (!any) {
		sse.send({ type: "_unknown_run", runId });
	}
	sse.send({ type: "_run_end" });
	sse.close();
}
