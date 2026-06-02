/**
 * Default Centinel cron schedule.
 *
 * Names match the keys used by `doge.config.yaml`'s
 * `cron_schedule_overrides:` section so operators can tune per-city
 * without touching code:
 *
 *   cron_schedule_overrides:
 *     investigator_tick: "0 *\/2 * * *"
 *     watch_runner:      "0 4 * * *"
 *
 * The override key (snake_case in YAML) is normalized to kebab-case here.
 * Job `name` is what shows up in the cron table and the CLI.
 */
import type { RoleName } from "../roles/types.js";

export interface CronJobDefinition {
	name: string;
	/** Cron expression (5-field, optionally 6-field with seconds — croner handles both). */
	cron: string;
	role: RoleName;
	prompt: string;
	/** The doge.config.yaml `cron_schedule_overrides:` key for this job. */
	overrideKey: string;
	/** Human-readable category for /status board. */
	category: "tick" | "lint" | "rollup" | "briefing" | "manifest";
}

/**
 * Default schedule. Operator overrides from `doge.config.yaml` are layered
 * on top of these at startup; see `cronTable.ts`.
 *
 * Schedules mirror the Hermes layout described in
 * `docs/AGENT_INVOCATION.md` / `docs/AGENT_ROSTER.md`.
 */
export const DEFAULT_CRON_JOBS: CronJobDefinition[] = [
	{
		name: "sitemap-lint",
		cron: "0 3 * * 1",
		role: "editor",
		prompt: "Weekly sitemap lint: re-walk the sitemap, flag new + broken URLs, update <wiki>/Sitemap/.",
		overrideKey: "sitemap_lint",
		category: "lint",
	},
	{
		name: "investigator-tick",
		cron: "0 */4 * * *",
		role: "investigator",
		prompt:
			"Drain <wiki>/_runtime/inbox/investigator/, run each pending task, and re-run any scheduled " +
			"investigations whose cadence is due. Write results to the investigation pages + Findings/raw/.",
		overrideKey: "investigator_tick",
		category: "tick",
	},
	{
		name: "archivist-tick",
		cron: "*/15 * * * *",
		role: "archivist",
		prompt:
			"Drain <wiki>/_runtime/inbox/archivist/. For each request, hash → vault → OCR → index → summarize. " +
			"Cross-check with the entity DB and flag discrepancies into the operator queue.",
		overrideKey: "archivist_tick",
		category: "tick",
	},
	{
		name: "data-reporter-tick",
		cron: "0 */6 * * *",
		role: "data-reporter",
		prompt:
			"Import any new entity-DB rows, run alias normalization, dedup candidates, and push merge candidates " +
			"to the operator queue when confidence < 0.9.",
		overrideKey: "data_reporter",
		category: "tick",
	},
	{
		name: "watch-runner-tick",
		cron: "0 */4 * * *",
		role: "watch-runner",
		prompt:
			"Scan sitemap diffs and new wiki pages against all active watches (preset + user-defined). " +
			"Drop hits into Findings/raw/. Auto-pause any watch that overflows its threshold.",
		overrideKey: "watch_runner",
		category: "tick",
	},
	{
		name: "vault-manifest",
		cron: "*/15 * * * *",
		role: "archivist",
		prompt: "Refresh <wiki>/_data/vault-manifest.json by walking the vault directory and verifying hashes.",
		overrideKey: "vault_manifest",
		category: "manifest",
	},
	{
		name: "huddle-rollup",
		cron: "0 18 * * *",
		role: "editor",
		prompt:
			"Roll up today's per-agent run logs into <wiki>/_runtime/huddle/<YYYY-MM-DD>.md following the " +
			"4-prompt Spotlight format (Did / Will / Blocked / New threads).",
		overrideKey: "huddle_rollup",
		category: "rollup",
	},
	{
		name: "briefings",
		cron: "0 9 * * 1",
		role: "editor",
		prompt:
			"Draft the weekly briefing: pull the week's findings, sitemap deltas, and investigation updates. " +
			"Write into <wiki>/Briefings/<YYYY-Www>.md for operator review before publishing.",
		overrideKey: "briefings",
		category: "briefing",
	},
];

/**
 * Apply `cron_schedule_overrides:` to the defaults. Unknown keys are
 * ignored (logged by the caller).
 */
export function applyOverrides(
	jobs: CronJobDefinition[],
	overrides: Record<string, string>,
): { jobs: CronJobDefinition[]; unknownKeys: string[] } {
	const byKey = new Map(jobs.map((j) => [j.overrideKey, j]));
	const out: CronJobDefinition[] = [];
	for (const j of jobs) {
		const overridden = overrides[j.overrideKey];
		out.push(overridden ? { ...j, cron: overridden } : j);
	}
	const unknownKeys = Object.keys(overrides).filter((k) => !byKey.has(k));
	return { jobs: out, unknownKeys };
}
