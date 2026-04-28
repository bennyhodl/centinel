import fs from "node:fs/promises";
import path from "node:path";
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
