import Link from "next/link";
import { computeStats, loadSitemap, sortEntries } from "@/lib/sitemap";
import { buildHostTrees, orderHosts } from "@/lib/sitemap-tree";
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
  const trees = buildHostTrees(doc);
  const pinnedHost = process.env.CENTINEL_HOST_DOMAIN ?? doc.domain ?? undefined;
  const orderedHosts = orderHosts([...trees.keys()], pinnedHost);

  const needsReview = sortEntries(
    doc.entries.filter((e) => e.status === "needs_review"),
  ).slice(0, 3);

  return (
    <section>
      <header className="mb-6">
        <h1 className="masthead text-3xl text-foreground">The Sitemap</h1>
        <hr className="rule-double" />
        <p className="text-sm text-muted-foreground italic">
          {doc.domain || "civic atlas"} &middot; generated {doc.generated_at}
        </p>
      </header>

      {/* Awaiting-review explainer */}
      {stats.needsReview > 0 && (
        <div className="mb-6 border-l-4 border-amber-700/60 bg-amber-50/60 px-4 py-3 text-sm text-foreground/80">
          <strong className="font-display text-amber-900">
            {stats.needsReview} pages awaiting review
          </strong>{" "}
          — the Cartographer flags newly-crawled URLs as{" "}
          <em>needs_review</em> until the operator approves or excludes them.
          Open a page below to triage; or{" "}
          <Link href="/sitemap/needs-review" className="text-primary underline">
            see the full queue
          </Link>
          .
        </div>
      )}

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

      {/* Hosts */}
      <div className="mb-8">
        <div className="section-header">Hosts</div>
        <div className="space-y-2">
          {orderedHosts.map((host) => {
            const tree = trees.get(host)!;
            const isPinned = host === pinnedHost;
            const topChildren = [...tree.root.children.values()]
              .sort((a, b) => b.descendantCount - a.descendantCount)
              .slice(0, 6);
            return (
              <Link
                key={host}
                href={`/sitemap/host/${encodeURIComponent(host)}`}
                className="block border border-border bg-card px-4 py-3 transition hover:bg-accent group"
              >
                <div className="flex items-baseline justify-between gap-3">
                  <div className="flex items-baseline gap-2">
                    {isPinned && (
                      <span className="border border-primary/30 bg-primary/5 px-1.5 py-0.5 font-smallcaps text-[0.55rem] tracking-[0.12em] text-primary uppercase">
                        primary
                      </span>
                    )}
                    <span className="font-mono text-sm text-foreground group-hover:text-primary transition-colors">
                      {host}
                    </span>
                  </div>
                  <span className="font-display text-lg font-bold">
                    {tree.total}
                  </span>
                </div>
                {topChildren.length > 0 && (
                  <div className="mt-2 flex flex-wrap gap-1.5 text-[0.65rem] text-muted-foreground">
                    {topChildren.map((c) => (
                      <span
                        key={c.segment}
                        className="border border-border bg-secondary px-1.5 py-0.5"
                      >
                        /{c.segment}{" "}
                        <span className="text-foreground/50">
                          {c.descendantCount}
                        </span>
                      </span>
                    ))}
                  </div>
                )}
              </Link>
            );
          })}
        </div>
      </div>

      {/* Operator queue: needs review (compact) */}
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
