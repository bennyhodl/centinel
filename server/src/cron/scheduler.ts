/**
 * In-process cron scheduler.
 *
 * Wraps `croner` and fires `runRole()` for each entry in the CronTable.
 * The whole point is that cron-driven runs go through *exactly* the same
 * `runRole()` code path that HTTP and CLI use — no parallel execution
 * surface.
 *
 * Concurrency policy: skip-if-running. If a job's previous run is still
 * active when the next tick fires, the tick is skipped (logged + status
 * recorded). Operator can override by hitting POST /cron/jobs/:name/fire
 * which will run regardless.
 */
import { Cron } from "croner";
import type { ServerConfig } from "../config.js";
import type { DogeConfig } from "../dogeConfig.js";
import { getRole } from "../roles/registry.js";
import type { RunSource } from "../roles/types.js";
import { runRole } from "../runtime/runRole.js";
import type { RunStore } from "../runtime/runStore.js";
import { CronTable, type CronJobEntry } from "./cronTable.js";

export interface SchedulerDeps {
	config: ServerConfig;
	store: RunStore;
	table: CronTable;
	doge: DogeConfig;
}

interface ScheduledHandle {
	job: Cron;
	inFlight: boolean;
}

export class Scheduler {
	private handles = new Map<string, ScheduledHandle>();

	constructor(private deps: SchedulerDeps) {}

	/**
	 * (Re)build the croner job set from the current CronTable contents.
	 * Safe to call after any table mutation — old jobs are stopped, new
	 * jobs are scheduled, paused jobs are honored.
	 */
	rebuild(): void {
		// Tear down anything we currently hold.
		for (const h of this.handles.values()) h.job.stop();
		this.handles.clear();

		const tz = this.deps.doge.city.timezone;
		for (const entry of this.deps.table.all()) {
			try {
				const job = new Cron(
					entry.cron,
					{
						name: entry.name,
						paused: entry.paused,
						protect: true,
						...(tz ? { timezone: tz } : {}),
					},
					() => {
						void this.fireFromCron(entry.name);
					},
				);
				this.handles.set(entry.name, { job, inFlight: false });
			} catch (err) {
				console.warn(
					`[centinel] cron entry "${entry.name}" rejected: ${err instanceof Error ? err.message : err}`,
				);
			}
		}
	}

	async shutdown(): Promise<void> {
		for (const h of this.handles.values()) h.job.stop();
		this.handles.clear();
	}

	pause(name: string): boolean {
		const updated = this.deps.table.setPaused(name, true);
		const h = this.handles.get(name);
		if (h) h.job.pause();
		return Boolean(updated);
	}

	resume(name: string): boolean {
		const updated = this.deps.table.setPaused(name, false);
		const h = this.handles.get(name);
		if (h) h.job.resume();
		return Boolean(updated);
	}

	pauseAll(): number {
		const n = this.deps.table.pauseAll();
		for (const h of this.handles.values()) h.job.pause();
		return n;
	}

	resumeAll(): number {
		const n = this.deps.table.resumeAll();
		for (const h of this.handles.values()) h.job.resume();
		return n;
	}

	listStatus(): Array<
		CronJobEntry & {
			nextRunAt: string | null;
			isBusy: boolean;
		}
	> {
		const out: Array<CronJobEntry & { nextRunAt: string | null; isBusy: boolean }> = [];
		for (const entry of this.deps.table.all()) {
			const h = this.handles.get(entry.name);
			const nextRun = h?.job.nextRun();
			out.push({
				...entry,
				nextRunAt: nextRun ? nextRun.toISOString() : null,
				isBusy: Boolean(h && h.inFlight),
			});
		}
		return out;
	}

	/**
	 * Manually fire a job. Goes through the same machinery as a scheduled
	 * tick but ignores the paused flag. Returns the runId immediately;
	 * caller can use /runs/:id/events to tail.
	 *
	 * Honors skip-if-running unless `force` is true.
	 */
	async fireManual(
		name: string,
		options: { source?: RunSource; force?: boolean; runId?: string } = {},
	): Promise<{ runId: string; skipped?: false } | { skipped: true; reason: string }> {
		const entry = this.deps.table.get(name);
		if (!entry) {
			throw new Error(`unknown cron job: ${name}`);
		}
		const handle = this.handles.get(name);
		if (!options.force && handle?.inFlight) {
			return { skipped: true, reason: "previous run still in_flight" };
		}
		return await this.executeJob(entry, options.source ?? "cron", options.runId);
	}

	/** Cron-triggered fire path. Honors skip-if-running. */
	private async fireFromCron(name: string): Promise<void> {
		const entry = this.deps.table.get(name);
		if (!entry || entry.paused) return;
		const handle = this.handles.get(name);
		if (handle?.inFlight) {
			console.warn(`[centinel] cron "${name}" skipped: previous run still in_flight`);
			this.deps.table.recordRun(name, { lastStatus: "skipped" });
			return;
		}
		try {
			await this.executeJob(entry, "cron");
		} catch (err) {
			console.error(`[centinel] cron "${name}" execution error:`, err);
		}
	}

	private async executeJob(
		entry: CronJobEntry,
		source: RunSource,
		runId?: string,
	): Promise<{ runId: string }> {
		const handle = this.handles.get(entry.name);
		const role = getRole(this.deps.config, entry.role);
		if (!role) {
			const msg = `cron "${entry.name}" references unknown role "${entry.role}"`;
			this.deps.table.recordRun(entry.name, {
				lastFiredAt: new Date().toISOString(),
				lastStatus: "error",
				lastErrorMessage: msg,
			});
			throw new Error(msg);
		}

		if (handle) handle.inFlight = true;
		this.deps.table.recordRun(entry.name, {
			lastFiredAt: new Date().toISOString(),
			lastStatus: "in_progress",
			lastErrorMessage: undefined,
		});

		// Run async — but capture runId synchronously by pre-generating it
		// and passing it into runRole().
		const id = runId ?? crypto.randomUUID();
		void (async () => {
			try {
				const result = await runRole(this.deps, role, {
					prompt: entry.prompt,
					source,
					runId: id,
					context: { cronJob: entry.name, kind: entry.kind, slug: entry.investigationSlug },
				});
				this.deps.table.recordRun(entry.name, {
					lastRunId: result.runId,
					lastStatus: result.ok ? "ok" : "error",
					lastErrorMessage: result.errorMessage,
				});
			} catch (err) {
				this.deps.table.recordRun(entry.name, {
					lastRunId: id,
					lastStatus: "error",
					lastErrorMessage: err instanceof Error ? err.message : String(err),
				});
			} finally {
				if (handle) handle.inFlight = false;
			}
		})();

		return { runId: id };
	}
}
