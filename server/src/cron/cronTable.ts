/**
 * Persistent cron table.
 *
 * Stored as `.runtime/cron.json`. Format is intentionally simple — an
 * array of `CronJobEntry`. The server keeps it in sync via writeFile after
 * any mutation.
 *
 * Two kinds of entries coexist:
 *   - default jobs (from `schedule.ts`, layered with doge.config.yaml
 *     overrides). These are auto-seeded on first boot and re-conciled on
 *     every subsequent boot: if a new default appears it's added paused;
 *     existing operator state is never clobbered.
 *   - per-investigation jobs (kind: "investigation"). Created via
 *     POST /cron/investigations/:slug/register. Owned by the operator.
 */
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import type { ServerConfig } from "../config.js";
import type { RoleName } from "../roles/types.js";
import { applyOverrides, DEFAULT_CRON_JOBS, type CronJobDefinition } from "./schedule.js";

export type CronJobKind = "default" | "investigation";
export type CronJobStatus = "never" | "ok" | "error" | "in_progress" | "skipped";

export interface CronJobEntry {
	name: string;
	kind: CronJobKind;
	cron: string;
	role: RoleName;
	prompt: string;
	paused: boolean;
	/** For default jobs: the doge.config override key. */
	overrideKey?: string;
	/** Free-form tag for the /status board. */
	category?: string;
	/** Per-investigation tag if kind === "investigation". */
	investigationSlug?: string;
	lastFiredAt?: string;
	lastRunId?: string;
	lastStatus?: CronJobStatus;
	lastErrorMessage?: string;
}

export interface CronTableSeedResult {
	added: string[];
	reconciledScheduleChanges: Array<{ name: string; from: string; to: string }>;
	unknownOverrideKeys: string[];
}

export class CronTable {
	private entries: CronJobEntry[] = [];
	private path: string;
	constructor(config: ServerConfig) {
		this.path = resolve(config.runtimeDir, "cron.json");
	}

	load(): void {
		if (!existsSync(this.path)) {
			this.entries = [];
			return;
		}
		try {
			const parsed = JSON.parse(readFileSync(this.path, "utf8")) as unknown;
			this.entries = Array.isArray(parsed) ? (parsed as CronJobEntry[]) : [];
		} catch (err) {
			console.warn(`[centinel] failed to parse ${this.path}: ${err instanceof Error ? err.message : err}`);
			this.entries = [];
		}
	}

	save(): void {
		mkdirSync(dirname(this.path), { recursive: true });
		writeFileSync(this.path, JSON.stringify(this.entries, null, 2) + "\n", "utf8");
	}

	/**
	 * Seed missing default jobs (paused), apply schedule-only changes for
	 * known defaults (preserves pause/lastRun state). Does NOT touch
	 * investigation entries.
	 */
	seedDefaults(overrides: Record<string, string>): CronTableSeedResult {
		const { jobs, unknownKeys } = applyOverrides(DEFAULT_CRON_JOBS, overrides);
		const added: string[] = [];
		const reconciled: Array<{ name: string; from: string; to: string }> = [];

		for (const def of jobs) {
			const existing = this.entries.find((e) => e.name === def.name && e.kind === "default");
			if (!existing) {
				this.entries.push(this.fromDefinition(def));
				added.push(def.name);
				continue;
			}
			// Update the cron expression if the operator override changed,
			// preserving paused state + last-run history.
			if (existing.cron !== def.cron) {
				reconciled.push({ name: def.name, from: existing.cron, to: def.cron });
				existing.cron = def.cron;
			}
			// Always refresh prompt/role/category from definition so doc updates
			// flow without touching operator state.
			existing.prompt = def.prompt;
			existing.role = def.role;
			existing.category = def.category;
			existing.overrideKey = def.overrideKey;
		}

		this.save();
		return { added, reconciledScheduleChanges: reconciled, unknownOverrideKeys: unknownKeys };
	}

	private fromDefinition(def: CronJobDefinition): CronJobEntry {
		return {
			name: def.name,
			kind: "default",
			cron: def.cron,
			role: def.role,
			prompt: def.prompt,
			paused: true,
			overrideKey: def.overrideKey,
			category: def.category,
			lastStatus: "never",
		};
	}

	all(): CronJobEntry[] {
		return this.entries.slice();
	}

	get(name: string): CronJobEntry | undefined {
		return this.entries.find((e) => e.name === name);
	}

	upsertInvestigation(args: {
		slug: string;
		cron: string;
		prompt?: string;
		role?: RoleName;
	}): CronJobEntry {
		const name = `centinel-investigation-${args.slug}`;
		const prompt =
			args.prompt ??
			`Run investigation ${args.slug}: read <wiki>/Investigations/${args.slug}.md and append results per the YAML directives.`;
		const role = args.role ?? "investigator";
		const existing = this.entries.find((e) => e.name === name);
		if (existing) {
			existing.cron = args.cron;
			existing.prompt = prompt;
			existing.role = role;
			existing.paused = false;
		} else {
			this.entries.push({
				name,
				kind: "investigation",
				cron: args.cron,
				role,
				prompt,
				paused: false,
				investigationSlug: args.slug,
				lastStatus: "never",
			});
		}
		this.save();
		return this.get(name)!;
	}

	removeInvestigation(slug: string): boolean {
		const name = `centinel-investigation-${slug}`;
		const before = this.entries.length;
		this.entries = this.entries.filter((e) => e.name !== name);
		const removed = this.entries.length !== before;
		if (removed) this.save();
		return removed;
	}

	setPaused(name: string, paused: boolean): CronJobEntry | undefined {
		const e = this.get(name);
		if (!e) return undefined;
		e.paused = paused;
		this.save();
		return e;
	}

	pauseAll(): number {
		let n = 0;
		for (const e of this.entries) {
			if (!e.paused) {
				e.paused = true;
				n++;
			}
		}
		if (n) this.save();
		return n;
	}

	resumeAll(): number {
		let n = 0;
		for (const e of this.entries) {
			if (e.paused) {
				e.paused = false;
				n++;
			}
		}
		if (n) this.save();
		return n;
	}

	recordRun(
		name: string,
		patch: Partial<Pick<CronJobEntry, "lastFiredAt" | "lastRunId" | "lastStatus" | "lastErrorMessage">>,
	): void {
		const e = this.get(name);
		if (!e) return;
		Object.assign(e, patch);
		this.save();
	}
}
