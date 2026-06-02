/**
 * Cron-related HTTP routes.
 *
 *   GET    /cron/jobs                              list all jobs with status
 *   GET    /cron/jobs/:name                        single job status
 *   POST   /cron/jobs/:name/fire                   manually fire (returns { runId }) — SSE-tailable
 *   POST   /cron/jobs/:name/pause
 *   POST   /cron/jobs/:name/resume
 *   POST   /cron/pause-all
 *   POST   /cron/resume-all                        used by setup wizard step 7
 *   POST   /cron/investigations/:slug/register     { cron, prompt?, role? }
 *   DELETE /cron/investigations/:slug
 */
import type { IncomingMessage, ServerResponse } from "node:http";
import type { Scheduler } from "../cron/scheduler.js";
import { errorJson, HttpError, json, readJsonBody, startSse, wantsSse } from "./util.js";
import type { RunStore } from "../runtime/runStore.js";

export interface CronRoutesDeps {
	scheduler: Scheduler;
	store: RunStore;
}

export function handleListCronJobs(deps: CronRoutesDeps, _req: IncomingMessage, res: ServerResponse): void {
	json(res, 200, { ok: true, jobs: deps.scheduler.listStatus() });
}

export function handleGetCronJob(
	deps: CronRoutesDeps,
	name: string,
	_req: IncomingMessage,
	res: ServerResponse,
): void {
	const job = deps.scheduler.listStatus().find((j) => j.name === name);
	if (!job) {
		json(res, 404, { ok: false, error: "unknown_cron_job", name });
		return;
	}
	json(res, 200, job);
}

export async function handleFireCronJob(
	deps: CronRoutesDeps,
	name: string,
	req: IncomingMessage,
	res: ServerResponse,
): Promise<void> {
	let body: { force?: boolean } = {};
	try {
		body = await readJsonBody(req);
	} catch {
		// allow empty/non-JSON for plain POST
	}

	let fired: Awaited<ReturnType<Scheduler["fireManual"]>>;
	try {
		fired = await deps.scheduler.fireManual(name, { source: "http", force: body.force });
	} catch (err) {
		if (err instanceof Error && err.message.startsWith("unknown cron job")) {
			json(res, 404, { ok: false, error: "unknown_cron_job", name });
			return;
		}
		errorJson(res, err);
		return;
	}

	if ("skipped" in fired && fired.skipped) {
		json(res, 409, { ok: false, error: "skipped", reason: fired.reason, name });
		return;
	}

	const runId = fired.runId;

	if (wantsSse(req)) {
		const sse = startSse(req, res);
		sse.send({ type: "_run_announced", runId, cronJob: name });

		// Subscribe to live events; replay buffered if we missed any.
		const active = deps.store.getActive(runId);
		if (active) {
			for (const ev of active.buffered) sse.send(ev);
			const unsub = deps.store.subscribe(runId, (ev) => sse.send(ev));
			sse.onClose(unsub);
		} else {
			// runRole may have finished extremely fast — replay from disk.
			for await (const ev of deps.store.replayFromDisk(runId)) sse.send(ev);
			sse.send({ type: "_run_end", runId });
			sse.close();
		}
		return;
	}

	json(res, 202, { ok: true, name, runId });
}

export async function handlePauseCronJob(
	deps: CronRoutesDeps,
	name: string,
	_req: IncomingMessage,
	res: ServerResponse,
): Promise<void> {
	const ok = deps.scheduler.pause(name);
	if (!ok) {
		json(res, 404, { ok: false, error: "unknown_cron_job", name });
		return;
	}
	json(res, 200, { ok: true, name, paused: true });
}

export async function handleResumeCronJob(
	deps: CronRoutesDeps,
	name: string,
	_req: IncomingMessage,
	res: ServerResponse,
): Promise<void> {
	const ok = deps.scheduler.resume(name);
	if (!ok) {
		json(res, 404, { ok: false, error: "unknown_cron_job", name });
		return;
	}
	json(res, 200, { ok: true, name, paused: false });
}

export function handlePauseAll(deps: CronRoutesDeps, _req: IncomingMessage, res: ServerResponse): void {
	const changed = deps.scheduler.pauseAll();
	json(res, 200, { ok: true, paused: changed });
}

export function handleResumeAll(deps: CronRoutesDeps, _req: IncomingMessage, res: ServerResponse): void {
	const changed = deps.scheduler.resumeAll();
	json(res, 200, { ok: true, resumed: changed });
}

interface RegisterInvestigationBody {
	cron?: unknown;
	prompt?: unknown;
	role?: unknown;
}

export async function handleRegisterInvestigation(
	deps: CronRoutesDeps & { table: import("../cron/cronTable.js").CronTable },
	slug: string,
	req: IncomingMessage,
	res: ServerResponse,
): Promise<void> {
	let body: RegisterInvestigationBody;
	try {
		body = await readJsonBody(req);
	} catch (err) {
		errorJson(res, err);
		return;
	}
	if (typeof body.cron !== "string" || body.cron.trim() === "") {
		errorJson(res, new HttpError(400, "invalid_cron", "`cron` must be a non-empty cron expression"));
		return;
	}
	const entry = deps.table.upsertInvestigation({
		slug,
		cron: body.cron,
		prompt: typeof body.prompt === "string" ? body.prompt : undefined,
		role: typeof body.role === "string" ? (body.role as never) : undefined,
	});
	deps.scheduler.rebuild();
	json(res, 200, { ok: true, entry });
}

export function handleUnregisterInvestigation(
	deps: CronRoutesDeps & { table: import("../cron/cronTable.js").CronTable },
	slug: string,
	_req: IncomingMessage,
	res: ServerResponse,
): void {
	const removed = deps.table.removeInvestigation(slug);
	if (!removed) {
		json(res, 404, { ok: false, error: "unknown_investigation", slug });
		return;
	}
	deps.scheduler.rebuild();
	json(res, 200, { ok: true, slug });
}
