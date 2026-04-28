import fs from "node:fs/promises";
import path from "node:path";
import os from "node:os";

/**
 * Resolves the path to the EDITOR_PERSONA.md plan doc.
 * Override with TAMPA_DOGE_EDITOR_PERSONA_PATH; defaults to ~/plans/tampa-doge/EDITOR_PERSONA.md.
 *
 * NOTE: this is a project-plan doc, NOT a wiki page. It lives in the planning
 * repo on the operator's machine. On a forked install where the plan dir doesn't
 * exist, buildSystemPrompt() falls back to an embedded short persona string.
 */
function editorPersonaPath(): string {
  const raw =
    process.env.TAMPA_DOGE_EDITOR_PERSONA_PATH ??
    "~/plans/tampa-doge/EDITOR_PERSONA.md";
  if (raw.startsWith("~/")) return path.join(os.homedir(), raw.slice(2));
  return path.resolve(raw);
}

const PERSONA_PREFIX = `You are the Editor for Tampa-DOGE — head of the investigative unit, chat persona.
The following spec defines your role, the editorial firewall, your tools (function-calling
spec — informational here; tools are not yet wired in this build), and citation rules.
Read it carefully and operate within it.

`;

const FALLBACK_PERSONA = `You are the Editor for Tampa-DOGE — head of the investigative unit. You read the
wiki, the project DB, the vault, and the operator queue. You synthesize narrative
findings (drafts only — operator publishes), register investigations, tune watches,
and delegate legwork to specialist agents (Cartographer, Investigator, Archivist,
Data Reporter, Watch Runner).

CITATION IS MANDATORY. Every factual claim cites a source — a vault path
([[Vault/pdfs/...]]), a wiki page ([[Contractors/acme]]), or a methodology query
(per Q-2026-04-26-001). If you have no source, say "I don't have a source for
that. Want me to investigate?" Never guess. Never paraphrase from memory. On
civic data, hallucinations destroy this project's credibility.

You DO NOT publish, send email, file FOIAs, contact subjects, or touch source
protection. The human operator does those. Tone: direct, journalistic, no filler.`;

/**
 * Build the system prompt sent to the model on every chat request.
 * Reads EDITOR_PERSONA.md fresh each call (cheap; tens of KB).
 *
 * TODO: once Hermes' OpenAI-compatible endpoint exposes function-calling for
 * the Editor toolset (wiki_search, wiki_read, db_query, draft_finding,
 * register_investigation, file_inbox_message, delegate_task, …), wire the tool
 * definitions through here and pass them to chat.completions.create alongside
 * the system prompt. For v0.1 we send prompt + messages only and the model
 * hedges appropriately ("I would need to query the wiki for…").
 */
export async function buildSystemPrompt(): Promise<string> {
  try {
    const body = await fs.readFile(editorPersonaPath(), "utf8");
    // Strip the YAML frontmatter so the model sees the prose first.
    const stripped = body.replace(/^---\n[\s\S]*?\n---\n+/, "");
    const looksFirstPerson = /\byou are\b/i.test(stripped.slice(0, 500));
    return looksFirstPerson ? stripped : PERSONA_PREFIX + stripped;
  } catch {
    return FALLBACK_PERSONA;
  }
}

export { EDITOR_INTRO_MESSAGE } from "./editor-intro";
