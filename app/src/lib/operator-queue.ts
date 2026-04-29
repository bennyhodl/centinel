import fs from "node:fs/promises";
import path from "node:path";
import matter from "gray-matter";
import crypto from "node:crypto";
import { readMarkdown, safeResolve } from "./wiki";

export const QUEUE_BUCKETS = [
  "entity-merges",
  "watch-tuning",
  "findings-draft-aging",
  "broken-watches",
] as const;
export type QueueBucket = (typeof QUEUE_BUCKETS)[number];

export const QUEUE_BUCKET_LABELS: Record<QueueBucket, string> = {
  "entity-merges": "Entity merges",
  "watch-tuning": "Watch tuning",
  "findings-draft-aging": "Aging drafts",
  "broken-watches": "Broken watches",
};

export type QueueStatus = "open" | "resolved" | "dismissed" | string;

export interface QueueItem {
  slug: string;
  bucket: QueueBucket;
  title: string;
  status: QueueStatus;
  createdAt: string | null;
  ageMs: number | null;
  frontmatter: Record<string, unknown>;
  excerpt: string;
}

const QUEUE_ROOT = path.posix.join("_runtime", "operator-queue");

function excerpt(body: string, n = 220): string {
  const stripped = body
    .replace(/^#.*$/gm, "")
    .replace(/\[\[([^\]|\n]+?)(?:\|([^\]\n]+))?\]\]/g, (_m, t, l) => l ?? t)
    .replace(/[*_`>]/g, "")
    .trim();
  return stripped.length > n ? stripped.slice(0, n).trimEnd() + "…" : stripped;
}

async function listBucket(bucket: QueueBucket): Promise<QueueItem[]> {
  const dir = path.posix.join(QUEUE_ROOT, bucket);
  let abs: string;
  try {
    abs = safeResolve(dir);
  } catch {
    return [];
  }
  let entries: string[];
  try {
    entries = await fs.readdir(abs);
  } catch {
    return [];
  }
  const files = entries.filter((e) => e.endsWith(".md"));
  const items = await Promise.all(
    files.map(async (e) => {
      const rel = path.posix.join(dir, e);
      try {
        const doc = await readMarkdown(rel);
        const fm = doc.frontmatter;
        const createdRaw =
          (fm.created_at as string | undefined) ??
          (fm.opened_at as string | undefined) ??
          null;
        const created = createdRaw ? new Date(createdRaw) : null;
        const ageMs =
          created && !Number.isNaN(created.getTime())
            ? Date.now() - created.getTime()
            : null;
        return {
          slug: path.basename(e, ".md"),
          bucket,
          title:
            (fm.title as string | undefined) ?? path.basename(e, ".md"),
          status: ((fm.status as string | undefined) ?? "open") as QueueStatus,
          createdAt: createdRaw,
          ageMs,
          frontmatter: fm,
          excerpt: excerpt(doc.body),
        } satisfies QueueItem;
      } catch {
        return null;
      }
    }),
  );
  return items.filter((i): i is QueueItem => !!i);
}

export async function listQueue(): Promise<QueueItem[]> {
  const all = await Promise.all(QUEUE_BUCKETS.map(listBucket));
  return all.flat();
}

export async function listQueueGrouped(): Promise<
  { bucket: QueueBucket; label: string; items: QueueItem[] }[]
> {
  const all = await listQueue();
  return QUEUE_BUCKETS.map((bucket) => ({
    bucket,
    label: QUEUE_BUCKET_LABELS[bucket],
    items: all
      .filter((i) => i.bucket === bucket)
      .sort((a, b) => (b.ageMs ?? 0) - (a.ageMs ?? 0)),
  }));
}

export function formatAge(ms: number | null): string {
  if (ms == null || !Number.isFinite(ms)) return "—";
  const s = Math.max(0, Math.floor(ms / 1000));
  if (s < 60) return `${s}s ago`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ago`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}h ago`;
  const d = Math.floor(h / 24);
  if (d < 30) return `${d}d ago`;
  const mo = Math.floor(d / 30);
  return `${mo}mo ago`;
}

export const AGE_AMBER_THRESHOLD_MS = 7 * 24 * 60 * 60 * 1000;

// ─────────────────────────────────────────────────────────────────────────
// Resolution
// ─────────────────────────────────────────────────────────────────────────
//
// Two flavors per AGENT_INVOCATION.md Lane 2:
//
//   1. Pure bookkeeping (frontmatter edit only). The agent already reacts
//      to `status: rejected | dismissed | acknowledged` on its next tick.
//
//   2. Agent work required. We additionally drop a directive into the
//      target role's inbox (`<wiki>/_runtime/inbox/<role>/...`) and the
//      role's cron tick performs the actual work and replies via outbox.
//
// The web app NEVER does agent work directly. Every "approval" that
// requires DB or watch-config changes goes through the inbox.

export type Decision =
  | "approve" // entity-merge | watch-tuning — agent will perform the work
  | "reject"
  | "dismiss"
  | "acknowledge"
  | "snooze";

export interface ResolveOptions {
  decision: Decision;
  reason?: string;
  resolvedBy?: string; // operator identity if known; defaults to "operator"
  snoozeUntil?: string; // ISO date when decision === "snooze"
}

/**
 * Map (bucket, decision) → which agent role gets an inbox message, if any.
 * Returning null means pure bookkeeping — no inbox file.
 */
function inboxRoleFor(
  bucket: QueueBucket,
  decision: Decision,
): "data-reporter" | "watch-runner" | null {
  if (decision !== "approve") return null;
  if (bucket === "entity-merges") return "data-reporter";
  if (bucket === "watch-tuning") return "watch-runner";
  return null;
}

/** Map decision verb → terminal `status:` value written into the queue file. */
function terminalStatus(decision: Decision): string {
  switch (decision) {
    case "approve":
      return "approved"; // queued-for-agent; agent flips to "complete" when done
    case "reject":
      return "rejected";
    case "dismiss":
      return "dismissed";
    case "acknowledge":
      return "acknowledged";
    case "snooze":
      return "snoozed";
  }
}

function nowIso(): string {
  return new Date().toISOString();
}

function tsForFilename(d = new Date()): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  return (
    `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}` +
    `-${pad(d.getHours())}${pad(d.getMinutes())}`
  );
}

function shortHash(...parts: string[]): string {
  return crypto.createHash("sha256").update(parts.join("|")).digest("hex").slice(0, 8);
}

/**
 * Atomically rewrite the queue item's frontmatter with terminal status +
 * resolution audit fields. Body is preserved verbatim.
 */
async function patchQueueItemFrontmatter(
  bucket: QueueBucket,
  slug: string,
  patch: Record<string, unknown>,
): Promise<void> {
  const rel = path.posix.join(QUEUE_ROOT, bucket, `${slug}.md`);
  const abs = safeResolve(rel);
  let raw: string;
  try {
    raw = await fs.readFile(abs, "utf-8");
  } catch (e: unknown) {
    const err = e as NodeJS.ErrnoException;
    if (err && err.code === "ENOENT") {
      throw new Error(`Queue item not found: ${bucket}/${slug}`);
    }
    throw e;
  }
  const parsed = matter(raw);
  const fm = { ...parsed.data, ...patch };
  const next = matter.stringify(parsed.content, fm);
  const tmp = `${abs}.tmp`;
  await fs.writeFile(tmp, next, "utf-8");
  await fs.rename(tmp, abs);
}

/**
 * Drop a directive into the appropriate role's inbox so the agent performs
 * the operator-approved work on its next cron tick.
 */
async function writeInboxDirective(
  role: "data-reporter" | "watch-runner",
  bucket: QueueBucket,
  slug: string,
  item: QueueItem,
  opts: ResolveOptions,
): Promise<string> {
  const ts = tsForFilename();
  const hash = shortHash(role, bucket, slug, ts);
  const filename = `${ts}-operator-${bucket}-${hash}.md`;
  const rel = path.posix.join(
    "_runtime",
    "inbox",
    role,
    filename,
  );
  const abs = safeResolve(rel);
  await fs.mkdir(path.dirname(abs), { recursive: true });

  const directiveType =
    bucket === "entity-merges"
      ? "entity-merge-resolution"
      : bucket === "watch-tuning"
        ? "watch-tuning-apply"
        : "operator-directive";

  const fm = {
    id: `${ts}-${hash}`,
    type: directiveType,
    from: "operator",
    to: role,
    status: "pending",
    priority: "normal",
    created: nowIso(),
    references: {
      operator_queue: `_runtime/operator-queue/${bucket}/${slug}.md`,
      ...(item.frontmatter.references &&
      typeof item.frontmatter.references === "object"
        ? (item.frontmatter.references as Record<string, unknown>)
        : {}),
    },
    correlation_id: (item.frontmatter.id as string | undefined) ?? slug,
  } as Record<string, unknown>;

  // Compose body: directive + the operator's reason if provided + a pointer
  // back to the queue item so the agent reads the original context.
  const lines: string[] = [];
  lines.push(`# Operator directive: ${directiveType}`);
  lines.push("");
  lines.push(
    bucket === "entity-merges"
      ? `Operator approved entity merge from queue item \`${slug}\`. Apply the merge per the candidates listed in the queue item's frontmatter \`references.entities\`.`
      : bucket === "watch-tuning"
        ? `Operator approved watch tuning from queue item \`${slug}\`. Apply the tuning recommendation per the queue item.`
        : `Operator directive — see referenced queue item.`,
  );
  if (opts.reason) {
    lines.push("");
    lines.push("## Operator note");
    lines.push(opts.reason);
  }
  lines.push("");
  lines.push("## Queue item");
  lines.push(`See \`_runtime/operator-queue/${bucket}/${slug}.md\` for full context.`);
  lines.push("");
  lines.push("## Reply");
  lines.push(
    `When done, write a result to \`_runtime/outbox/${role}/<YYYY-MM>/\` with \`correlation_id: ${fm.correlation_id}\` and flip the queue item's \`status:\` from \`approved\` to \`complete\`.`,
  );

  const tmp = `${abs}.tmp`;
  await fs.writeFile(tmp, matter.stringify(lines.join("\n"), fm), "utf-8");
  await fs.rename(tmp, abs);
  return rel;
}

/**
 * Resolve one queue item.
 *
 * - Always: rewrites queue file frontmatter with terminal status + audit fields.
 * - Conditionally (decision === "approve" on a bucket needing agent work):
 *   also drops an inbox directive for the responsible role.
 *
 * Returns metadata about what was written.
 */
export interface ResolveResult {
  status: string;
  inboxPath: string | null;
  needsAgent: boolean;
}

export async function resolveQueueItem(
  bucket: QueueBucket,
  slug: string,
  opts: ResolveOptions,
): Promise<ResolveResult> {
  // Read the item first so we can include its references in the inbox file.
  const rel = path.posix.join(QUEUE_ROOT, bucket, `${slug}.md`);
  const item = await readQueueItem(bucket, slug, rel);
  if (!item) throw new Error(`Queue item not found: ${bucket}/${slug}`);

  const status = terminalStatus(opts.decision);
  const role = inboxRoleFor(bucket, opts.decision);

  let inboxPath: string | null = null;
  if (role) {
    inboxPath = await writeInboxDirective(role, bucket, slug, item, opts);
  }

  const patch: Record<string, unknown> = {
    status,
    resolved_at: nowIso(),
    resolved_by: opts.resolvedBy ?? "operator",
  };
  if (opts.reason) patch.resolution_reason = opts.reason;
  if (opts.decision === "snooze" && opts.snoozeUntil) {
    patch.snooze_until = opts.snoozeUntil;
  }
  if (inboxPath) patch.inbox_directive = inboxPath;

  await patchQueueItemFrontmatter(bucket, slug, patch);

  return { status, inboxPath, needsAgent: role !== null };
}

async function readQueueItem(
  bucket: QueueBucket,
  slug: string,
  rel: string,
): Promise<QueueItem | null> {
  try {
    safeResolve(rel);
    const doc = await readMarkdown(rel);
    return {
      slug,
      bucket,
      title: (doc.frontmatter.title as string | undefined) ?? slug,
      status: ((doc.frontmatter.status as string | undefined) ?? "open") as QueueStatus,
      createdAt: (doc.frontmatter.created_at as string | undefined) ?? null,
      ageMs: null,
      frontmatter: doc.frontmatter,
      excerpt: excerpt(doc.body),
    };
  } catch {
    return null;
  }
}
