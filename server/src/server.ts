#!/usr/bin/env node
/**
 * centinel-server — long-running Node process owning every agent role,
 * the internal cron, the HTTP/SSE surface, and the run log.
 *
 * Phase 2 routes:
 *   GET  /health
 *
 *   POST /run/:role                        sync JSON or SSE
 *   GET  /runs
 *   GET  /runs/:id
 *   GET  /runs/:id/events                  SSE replay + tail
 *
 *   GET  /cron/jobs
 *   GET  /cron/jobs/:name
 *   POST /cron/jobs/:name/fire             sync JSON or SSE
 *   POST /cron/jobs/:name/pause
 *   POST /cron/jobs/:name/resume
 *   POST /cron/pause-all
 *   POST /cron/resume-all                  setup wizard step 7
 *   POST /cron/investigations/:slug/register
 *   DELETE /cron/investigations/:slug
 *
 * See docs/PI_MIGRATION_PLAN.md.
 */

import { createServer, type IncomingMessage, type ServerResponse } from "node:http";
import { loadConfig, type ServerConfig } from "./config.js";
import { loadDogeConfig, type DogeConfig } from "./dogeConfig.js";
import { listRoleNames } from "./roles/registry.js";
import { RunStore } from "./runtime/runStore.js";
import { CronTable } from "./cron/cronTable.js";
import { Scheduler } from "./cron/scheduler.js";
import { ChatSessions } from "./chat/chatSessions.js";
import { getRole } from "./roles/registry.js";
import {
	handleGetRun,
	handleListRuns,
	handlePostRun,
	handleRunEvents,
} from "./http/runRoutes.js";
import {
	handleFireCronJob,
	handleGetCronJob,
	handleListCronJobs,
	handlePauseAll,
	handlePauseCronJob,
	handleRegisterInvestigation,
	handleResumeAll,
	handleResumeCronJob,
	handleUnregisterInvestigation,
} from "./http/cronRoutes.js";
import {
	handleAbortChatSession,
	handleListChatSessionsActive,
	handleListChatSessionsOnDisk,
	handlePostChat,
} from "./http/chatRoutes.js";
import { errorJson, json } from "./http/util.js";

const VERSION = "0.1.0";
const PHASE = "phase-3-editor-chat";
const STARTED_AT = new Date();

interface RouterDeps {
	config: ServerConfig;
	doge: DogeConfig;
	store: RunStore;
	table: CronTable;
	scheduler: Scheduler;
	chat: ChatSessions;
}

function buildHealth(deps: RouterDeps) {
	const jobs = deps.scheduler.listStatus();
	const paused = jobs.filter((j) => j.paused).length;
	const inFlight = jobs.filter((j) => j.isBusy).length;
	const next = jobs
		.filter((j) => !j.paused && j.nextRunAt)
		.map((j) => j.nextRunAt!)
		.sort()[0];
	return {
		ok: true as const,
		service: "centinel-server" as const,
		version: VERSION,
		phase: PHASE,
		startedAt: STARTED_AT.toISOString(),
		uptimeSeconds: Math.round((Date.now() - STARTED_AT.getTime()) / 1000),
		rolesWired: listRoleNames(),
		city: deps.doge.city,
		cron: {
			total: jobs.length,
			paused,
			active: jobs.length - paused,
			inFlight,
			nextDueAt: next ?? null,
		},
		runtimeDir: deps.config.runtimeDir,
	};
}

interface Route {
	method: "GET" | "POST" | "DELETE";
	pattern: string[];
	handle: (
		params: Record<string, string>,
		req: IncomingMessage,
		res: ServerResponse,
		deps: RouterDeps,
	) => Promise<void> | void;
}

const routes: Route[] = [
	{
		method: "GET",
		pattern: ["health"],
		handle: (_p, _req, res, deps) => json(res, 200, buildHealth(deps)),
	},

	// /run/* — Phase 1
	{
		method: "POST",
		pattern: ["run", ":role"],
		handle: (p, req, res, deps) => handlePostRun(deps, p.role!, req, res),
	},
	{
		method: "GET",
		pattern: ["runs"],
		handle: (_p, req, res, deps) => handleListRuns(deps, req, res),
	},
	{
		method: "GET",
		pattern: ["runs", ":id"],
		handle: (p, req, res, deps) => handleGetRun(deps, p.id!, req, res),
	},
	{
		method: "GET",
		pattern: ["runs", ":id", "events"],
		handle: (p, req, res, deps) => handleRunEvents(deps, p.id!, req, res),
	},

	// /cron/* — Phase 2
	{
		method: "GET",
		pattern: ["cron", "jobs"],
		handle: (_p, req, res, deps) => handleListCronJobs(deps, req, res),
	},
	{
		method: "GET",
		pattern: ["cron", "jobs", ":name"],
		handle: (p, req, res, deps) => handleGetCronJob(deps, p.name!, req, res),
	},
	{
		method: "POST",
		pattern: ["cron", "jobs", ":name", "fire"],
		handle: (p, req, res, deps) => handleFireCronJob(deps, p.name!, req, res),
	},
	{
		method: "POST",
		pattern: ["cron", "jobs", ":name", "pause"],
		handle: (p, req, res, deps) => handlePauseCronJob(deps, p.name!, req, res),
	},
	{
		method: "POST",
		pattern: ["cron", "jobs", ":name", "resume"],
		handle: (p, req, res, deps) => handleResumeCronJob(deps, p.name!, req, res),
	},
	{
		method: "POST",
		pattern: ["cron", "pause-all"],
		handle: (_p, req, res, deps) => handlePauseAll(deps, req, res),
	},
	{
		method: "POST",
		pattern: ["cron", "resume-all"],
		handle: (_p, req, res, deps) => handleResumeAll(deps, req, res),
	},
	{
		method: "POST",
		pattern: ["cron", "investigations", ":slug", "register"],
		handle: (p, req, res, deps) => handleRegisterInvestigation(deps, p.slug!, req, res),
	},
	{
		method: "DELETE",
		pattern: ["cron", "investigations", ":slug"],
		handle: (p, req, res, deps) => handleUnregisterInvestigation(deps, p.slug!, req, res),
	},

	// /chat/* — Phase 3
	{
		method: "POST",
		pattern: ["chat"],
		handle: (_p, req, res, deps) => handlePostChat(deps, req, res),
	},
	{
		method: "GET",
		pattern: ["chat", "sessions"],
		handle: (_p, req, res, deps) => handleListChatSessionsOnDisk(deps, req, res),
	},
	{
		method: "GET",
		pattern: ["chat", "sessions", "active"],
		handle: (_p, req, res, deps) => handleListChatSessionsActive(deps, req, res),
	},
	{
		method: "POST",
		pattern: ["chat", "sessions", ":id", "abort"],
		handle: (p, req, res, deps) => handleAbortChatSession(deps, p.id!, req, res),
	},
];

function splitPath(url: string): string[] {
	const path = url.split("?")[0] ?? "/";
	return path.split("/").filter(Boolean);
}

function match(route: Route, method: string, segs: string[]): Record<string, string> | undefined {
	if (route.method !== method) return undefined;
	if (route.pattern.length !== segs.length) return undefined;
	const params: Record<string, string> = {};
	for (let i = 0; i < segs.length; i++) {
		const pat = route.pattern[i]!;
		const seg = segs[i]!;
		if (pat.startsWith(":")) params[pat.slice(1)] = decodeURIComponent(seg);
		else if (pat !== seg) return undefined;
	}
	return params;
}

async function handle(req: IncomingMessage, res: ServerResponse, deps: RouterDeps): Promise<void> {
	const url = req.url ?? "/";
	const method = (req.method ?? "GET").toUpperCase();
	const segs = splitPath(url);

	for (const route of routes) {
		const params = match(route, method, segs);
		if (params) {
			try {
				await route.handle(params, req, res, deps);
			} catch (err) {
				if (!res.headersSent) errorJson(res, err);
				else res.end();
			}
			return;
		}
	}

	json(res, 404, { ok: false, error: "not_found", path: url });
}

export async function start(config: ServerConfig = loadConfig()): Promise<{ close: () => Promise<void> }> {
	const doge = loadDogeConfig(config);
	const store = new RunStore(config);
	const table = new CronTable(config);
	table.load();
	const seed = table.seedDefaults(doge.cronScheduleOverrides);
	if (seed.added.length) console.log(`[centinel-server] seeded cron defaults: ${seed.added.join(", ")}`);
	if (seed.reconciledScheduleChanges.length) {
		for (const c of seed.reconciledScheduleChanges) {
			console.log(`[centinel-server] schedule changed for ${c.name}: ${c.from} -> ${c.to}`);
		}
	}
	if (seed.unknownOverrideKeys.length) {
		console.warn(
			`[centinel-server] doge.config.yaml cron_schedule_overrides: unknown keys: ${seed.unknownOverrideKeys.join(", ")}`,
		);
	}

	const scheduler = new Scheduler({ config, store, table, doge });
	scheduler.rebuild();

	const editorRoleConfig = getRole(config, "editor");
	if (!editorRoleConfig) throw new Error("editor role not registered");
	const chat = new ChatSessions({ config, store, editorRole: editorRoleConfig });

	const deps: RouterDeps = { config, doge, store, table, scheduler, chat };
	const server = createServer((req, res) => {
		handle(req, res, deps).catch((err: unknown) => {
			console.error("[centinel-server] unhandled:", err);
			if (!res.headersSent) {
				res.statusCode = 500;
				res.end();
			}
		});
	});

	await new Promise<void>((res, rej) => {
		server.once("error", rej);
		server.listen(config.port, config.host, () => {
			server.removeListener("error", rej);
			res();
		});
	});

	console.log(`[centinel-server] listening on http://${config.host}:${config.port}`);
	console.log(`[centinel-server] ${PHASE} — roles: ${listRoleNames().join(", ")}`);
	console.log(
		`[centinel-server] cron: ${scheduler.listStatus().length} jobs (${scheduler.listStatus().filter((j) => j.paused).length} paused)`,
	);
	if (doge.sourcePath) console.log(`[centinel-server] doge.config.yaml: ${doge.sourcePath}`);

	const close = async () => {
		await scheduler.shutdown();
		await chat.dispose();
		await new Promise<void>((res) => server.close(() => res()));
	};

	const shutdown = (sig: string) => {
		console.log(`[centinel-server] received ${sig}, shutting down`);
		close().then(() => process.exit(0));
		setTimeout(() => process.exit(1), 5000).unref();
	};
	process.on("SIGINT", () => shutdown("SIGINT"));
	process.on("SIGTERM", () => shutdown("SIGTERM"));

	return { close };
}

const invokedAsScript = import.meta.url === `file://${process.argv[1]}`;
if (invokedAsScript) {
	start().catch((err: unknown) => {
		console.error("[centinel-server] failed to start:", err);
		process.exit(1);
	});
}
