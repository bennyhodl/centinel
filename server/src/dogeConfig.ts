/**
 * Loader for per-city operator config (`doge.config.yaml`).
 *
 * Phase 2 reads three sections:
 *   - city.timezone               (for cron scheduling)
 *   - cron_schedule_overrides     (layer over default schedule)
 *   - confidential_investigations (consumed by /status filtering later)
 *
 * Missing file = empty config. Malformed file = log a warning + use defaults.
 */
import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { parse as parseYaml } from "yaml";
import type { ServerConfig } from "./config.js";

export interface DogeConfig {
	city: {
		name?: string;
		slug?: string;
		domain?: string;
		timezone?: string;
	};
	/** Operator's wiki root (raw string from yaml, may contain `~` / env vars). */
	wikiPath?: string;
	/** Map of cron job name → cron expression overriding the default. */
	cronScheduleOverrides: Record<string, string>;
	confidentialInvestigations: string[];
	/** Absolute path of the file we loaded from, if any. */
	sourcePath?: string;
}

const EMPTY: DogeConfig = {
	city: {},
	cronScheduleOverrides: {},
	confidentialInvestigations: [],
};

export function loadDogeConfig(config: ServerConfig): DogeConfig {
	const path = resolve(config.repoRoot, "doge.config.yaml");
	if (!existsSync(path)) return { ...EMPTY };
	try {
		const text = readFileSync(path, "utf8");
		const parsed = parseYaml(text) as Record<string, unknown> | undefined;
		if (!parsed || typeof parsed !== "object") return { ...EMPTY };

		const city = (parsed.city as Record<string, unknown> | undefined) ?? {};
		const overrides = (parsed.cron_schedule_overrides as Record<string, unknown> | undefined) ?? {};
		const confidential = (parsed.confidential_investigations as unknown[] | undefined) ?? [];
		const wiki = (parsed.wiki as Record<string, unknown> | undefined) ?? {};

		const cronScheduleOverrides: Record<string, string> = {};
		for (const [k, v] of Object.entries(overrides)) {
			if (typeof v === "string" && v.trim() !== "") cronScheduleOverrides[k] = v;
		}

		return {
			city: {
				name: typeof city.name === "string" ? city.name : undefined,
				slug: typeof city.slug === "string" ? city.slug : undefined,
				domain: typeof city.domain === "string" ? city.domain : undefined,
				timezone: typeof city.timezone === "string" ? city.timezone : undefined,
			},
			wikiPath: typeof wiki.path === "string" ? wiki.path : undefined,
			cronScheduleOverrides,
			confidentialInvestigations: confidential.filter((s): s is string => typeof s === "string"),
			sourcePath: path,
		};
	} catch (err) {
		console.warn(`[centinel] failed to parse ${path}: ${err instanceof Error ? err.message : err}`);
		return { ...EMPTY };
	}
}
