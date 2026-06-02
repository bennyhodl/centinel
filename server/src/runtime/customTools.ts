/**
 * Stub implementations of the Centinel-specific tools the existing skills
 * reference (qmd_search, db_query, vault_put, web_fetch).
 *
 * Phase 1: every tool returns a structured "not_yet_implemented" payload.
 * The point is to wire the tool surface so the investigator can be invoked
 * end-to-end and we can observe, via the run log, which tool calls it
 * attempted. Each stub will be replaced with a real implementation in
 * subsequent phases (see docs/PI_MIGRATION_PLAN.md "What we lose").
 */
import { Type } from "typebox";
import { defineTool, type ToolDefinition } from "@mariozechner/pi-coding-agent";

function stubResult(tool: string, params: unknown) {
	return {
		content: [
			{
				type: "text" as const,
				text:
					`[centinel] ${tool} is not yet implemented in the pi-agent runtime.\n` +
					`Phase 1 stub — captured the call so the run log shows intent.\n` +
					`Params: ${JSON.stringify(params)}`,
			},
		],
		details: { tool, params, status: "not_yet_implemented" as const },
		isError: false as const,
	};
}

export const qmdSearchTool: ToolDefinition = defineTool({
	name: "qmd_search",
	label: "QMD search",
	description:
		"Search the wiki via BM25 + vector + reranker. Required answer source per docs/EDITOR_ANSWER_SOURCES.md. " +
		"Returns wiki page hits with scores and snippets.",
	parameters: Type.Object({
		query: Type.String({ description: "Free-text query." }),
		limit: Type.Optional(Type.Number({ description: "Max hits to return (default 10).", minimum: 1, maximum: 50 })),
	}),
	execute: async (_id, params) => stubResult("qmd_search", params),
});

export const dbQueryTool: ToolDefinition = defineTool({
	name: "db_query",
	label: "DB query",
	description:
		"Run a read-only SQL query against the city wiki's SQLite DB (<wiki>/_data/<city>.db). " +
		"Returns rows as JSON.",
	parameters: Type.Object({
		sql: Type.String({ description: "SQL query (SELECT only)." }),
		params: Type.Optional(Type.Array(Type.Union([Type.String(), Type.Number(), Type.Null()]))),
	}),
	execute: async (_id, params) => stubResult("db_query", params),
});

export const vaultPutTool: ToolDefinition = defineTool({
	name: "vault_put",
	label: "Vault put",
	description:
		"Hash-and-store a public document into the city's vault. Returns the content-addressed vault path " +
		"the Archivist would later OCR + summarize.",
	parameters: Type.Object({
		url: Type.String({ description: "Source URL of the document (public only)." }),
		kind: Type.Optional(Type.String({ description: "Optional document kind hint (pdf, html, transcript, image)." })),
		investigation: Type.Optional(Type.String({ description: "Investigation slug this artifact belongs to." })),
	}),
	execute: async (_id, params) => stubResult("vault_put", params),
});

export const webFetchTool: ToolDefinition = defineTool({
	name: "web_fetch",
	label: "Web fetch",
	description:
		"Fetch a public URL (HTML or PDF) and return its textual content. Public surfaces only — never used " +
		"to contact named subjects or non-public APIs.",
	parameters: Type.Object({
		url: Type.String({ description: "Public URL to fetch." }),
		mode: Type.Optional(
			Type.Union([Type.Literal("text"), Type.Literal("markdown"), Type.Literal("raw")], {
				description: "Output mode (default: markdown).",
			}),
		),
	}),
	execute: async (_id, params) => stubResult("web_fetch", params),
});

export const centinelCustomTools: ToolDefinition[] = [qmdSearchTool, dbQueryTool, vaultPutTool, webFetchTool];
