import Link from "next/link";
import { computeStats, loadSitemap, sortEntries } from "@/lib/sitemap";
import { SitemapEntryCard, StatusPill } from "./_components/SitemapEntryCard";
import { SitemapEmptyState } from "./_components/EmptyState";

export const dynamic = "force-dynamic";

export default async function SitemapPage() {
  const doc = await loadSitemap();

  if (!doc) {
    return (
      <section>
        <header className="mb-6">
          <h1 className="masthead text-3xl text-foreground">The Sitemap</h1>
          <hr className="rule-double" />
          <p className="text-sm text-muted-foreground italic">
            The labeled map of the city&apos;s .gov surface.
          </p>
        </header>
        <SitemapEmptyState />
      </section>
    );
  }

  const stats = computeStats(doc);
  const needsReview = sortEntries(
    doc.entries.filter((e) => e.status === "needs_review"),
  ).slice(0, 5);

  const sortedTypes = Object.entries(stats.byType).sort(
    (a, b) => b[1] - a[1],
  );

  return (
    <section>
      <header className="mb-6">
        <h1 className="masthead text-3xl text-foreground">The Sitemap</h1>
        <hr className="rule-double" />
        <p className="text-sm text-muted-foreground italic">
          {doc.domain} &middot; generated {doc.generated_at}
        </p>
      </header>

      {/* Stat strip */}
      <div className="grid grid-cols-2 gap-4 sm:grid-cols-4 mb-8">
        <Stat label="Total URLs" value={stats.total} />
        <Stat label="Active" value={stats.byStatus.active} tone="emerald" />
        <Stat
          label="Needs Review"
          value={stats.needsReview}
          tone="amber"
          href="/sitemap/needs-review"
        />
        <Stat label="Broken" value={stats.broken} tone="red" href="/sitemap/broken" />
      </div>

      {/* By type */}
      <div className="mb-8">
        <div className="section-header">By Type</div>
        <div className="grid grid-cols-2 gap-3 sm:grid-cols-3 md:grid-cols-4">
          {sortedTypes.map(([type, count]) => (
            <Link
              key={type}
              href={`/sitemap/type/${type}`}
              className="border border-border bg-card px-4 py-3 transition hover:bg-accent group"
            >
              <div className="font-smallcaps text-[0.6rem] tracking-[0.15em] text-muted-foreground">
                {type}
              </div>
              <div className="font-display text-xl font-bold mt-1 group-hover:text-primary transition-colors">
                {count}
              </div>
            </Link>
          ))}
        </div>
      </div>

      {/* Operator queue: needs review */}
      {needsReview.length > 0 && (
        <div className="mb-8">
          <div className="flex items-baseline justify-between mb-3">
            <div className="section-header flex-1">Awaiting Review</div>
            <Link
              href="/sitemap/needs-review"
              className="text-xs text-primary hover:underline italic ml-4"
            >
              see all {stats.needsReview} &rarr;
            </Link>
          </div>
          <div className="space-y-3">
            {needsReview.map((e) => (
              <SitemapEntryCard key={e.url} entry={e} />
            ))}
          </div>
        </div>
      )}

      {/* Status legend */}
      <footer className="flex flex-wrap items-center gap-3 border-t-2 border-foreground/15 pt-4 text-xs text-muted-foreground">
        <span className="italic">Statuses:</span>
        <StatusPill status="active" />
        <StatusPill status="needs_review" />
        <StatusPill status="broken" />
        <StatusPill status="excluded" />
      </footer>
    </section>
  );
}

function Stat({
  label,
  value,
  tone,
  href,
}: {
  label: string;
  value: number;
  tone?: "emerald" | "amber" | "red";
  href?: string;
}) {
  const toneClass =
    tone === "emerald"
      ? "text-emerald-800"
      : tone === "amber"
        ? "text-amber-800"
        : tone === "red"
          ? "text-red-800"
          : "text-foreground";
  const inner = (
    <div className="border border-border bg-card p-4 text-center">
      <div className="font-smallcaps text-[0.6rem] tracking-[0.15em] text-muted-foreground">
        {label}
      </div>
      <div className={`mt-1 font-display text-2xl font-bold ${toneClass}`}>
        {value}
      </div>
    </div>
  );
  return href ? (
    <Link href={href} className="block transition hover:opacity-80">
      {inner}
    </Link>
  ) : (
    inner
  );
}
