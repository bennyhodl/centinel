import Link from "next/link";
import type { FindingSummary } from "@/lib/findings";

function relTime(input: string | undefined): string {
  if (!input) return "";
  const t = Date.parse(input);
  if (Number.isNaN(t)) return "";
  const dt = Date.now() - t;
  if (dt < 60_000) return "just now";
  if (dt < 3_600_000) return `${Math.floor(dt / 60_000)}m ago`;
  if (dt < 86_400_000) return `${Math.floor(dt / 3_600_000)}h ago`;
  return `${Math.floor(dt / 86_400_000)}d ago`;
}

const STACK_TONE: Record<string, string> = {
  draft: "border-amber-400/50 bg-amber-50 text-amber-900",
  published: "border-emerald-500/50 bg-emerald-50 text-emerald-900",
  raw: "border-zinc-400/50 bg-zinc-50 text-zinc-900",
};

export function RecentFindings({ items }: { items: FindingSummary[] }) {
  if (items.length === 0) {
    return (
      <div className="border border-dashed border-border bg-card/60 p-4 text-xs text-muted-foreground italic">
        No findings yet. They land here as agents complete runs.
      </div>
    );
  }
  return (
    <ol className="space-y-2">
      {items.map((f) => {
        const fm = f.frontmatter as Record<string, unknown>;
        const title =
          (typeof fm.title === "string" && fm.title) ||
          (typeof fm.headline === "string" && fm.headline) ||
          f.slug;
        const dateField =
          f.stack === "draft"
            ? "drafted_at"
            : f.stack === "published"
              ? "published_at"
              : "generated_at";
        const dateRaw = fm[dateField] ?? fm.date;
        const dateStr = typeof dateRaw === "string" ? dateRaw : "";

        const inv =
          typeof fm.investigation === "string" ? fm.investigation : null;

        const tone = STACK_TONE[f.stack] ?? "border-border bg-secondary";

        return (
          <li key={`${f.stack}/${f.slug}`} className="border border-border bg-card p-3">
            <div className="flex flex-wrap items-baseline gap-2">
              <span
                className={`border px-1.5 py-0.5 font-smallcaps text-[0.55rem] tracking-[0.12em] uppercase ${tone}`}
              >
                {f.stack}
              </span>
              {dateStr && (
                <span className="text-[0.65rem] text-muted-foreground">
                  {relTime(dateStr)}
                </span>
              )}
            </div>
            <Link
              href={`/findings/${f.slug}`}
              className="mt-1 block font-display text-sm font-semibold text-foreground hover:text-primary"
            >
              {title}
            </Link>
            {f.excerpt && (
              <p className="mt-1 text-xs text-muted-foreground leading-relaxed">
                {f.excerpt}
              </p>
            )}
            {inv && (
              <Link
                href={`/investigations/${inv}`}
                className="mt-1.5 inline-block font-mono text-[0.65rem] text-primary hover:underline"
              >
                ↪ {inv}
              </Link>
            )}
          </li>
        );
      })}
    </ol>
  );
}
