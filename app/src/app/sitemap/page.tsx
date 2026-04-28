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
          <h1 className="text-2xl font-semibold">Sitemap</h1>
          <p className="mt-1 text-sm opacity-60">
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
    <section className="space-y-8">
      <header>
        <h1 className="text-2xl font-semibold">Sitemap</h1>
        <p className="mt-1 text-sm opacity-60">
          {doc.domain} · generated {doc.generated_at}
        </p>
      </header>

      {/* Stat strip */}
      <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
        <Stat label="total URLs" value={stats.total} />
        <Stat
          label="active"
          value={stats.byStatus.active}
          tone="emerald"
        />
        <Stat
          label="needs review"
          value={stats.needsReview}
          tone="amber"
          href="/sitemap/needs-review"
        />
        <Stat
          label="broken"
          value={stats.broken}
          tone="red"
          href="/sitemap/broken"
        />
      </div>

      {/* By type */}
      <div>
        <h2 className="mb-3 text-sm font-semibold uppercase tracking-wider opacity-60">
          By type
        </h2>
        <div className="grid grid-cols-2 gap-2 sm:grid-cols-3 md:grid-cols-4">
          {sortedTypes.map(([type, count]) => (
            <Link
              key={type}
              href={`/sitemap/type/${type}`}
              className="rounded-md border border-white/10 bg-white/[0.02] px-3 py-2 transition hover:border-tampa-cyan/40 hover:bg-white/[0.04]"
            >
              <div className="text-xs uppercase tracking-wider opacity-60">
                {type}
              </div>
              <div className="font-mono text-lg">{count}</div>
            </Link>
          ))}
        </div>
      </div>

      {/* Operator queue: needs review */}
      {needsReview.length > 0 && (
        <div>
          <div className="mb-3 flex items-baseline justify-between">
            <h2 className="text-sm font-semibold uppercase tracking-wider opacity-60">
              Awaiting review
            </h2>
            <Link
              href="/sitemap/needs-review"
              className="text-xs text-tampa-cyan hover:underline"
            >
              see all {stats.needsReview} →
            </Link>
          </div>
          <div className="grid gap-3">
            {needsReview.map((e) => (
              <SitemapEntryCard key={e.url} entry={e} />
            ))}
          </div>
        </div>
      )}

      {/* Status legend */}
      <footer className="flex flex-wrap items-center gap-3 border-t border-white/10 pt-4 text-xs opacity-70">
        <span>Statuses:</span>
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
      ? "text-emerald-400"
      : tone === "amber"
        ? "text-amber-400"
        : tone === "red"
          ? "text-red-400"
          : "text-white";
  const inner = (
    <div className="rounded-lg border border-white/10 bg-white/[0.02] p-4">
      <div className="text-xs uppercase tracking-wider opacity-60">
        {label}
      </div>
      <div className={`mt-1 font-mono text-2xl font-semibold ${toneClass}`}>
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
