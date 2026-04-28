import Link from "next/link";
import { notFound } from "next/navigation";
import {
  filterByStatus,
  filterByType,
  loadSitemap,
  searchEntries,
  sortEntries,
  SitemapEntryStatus,
  SitemapEntryType,
} from "@/lib/sitemap";
import { SitemapEntryCard } from "../_components/SitemapEntryCard";
import { SitemapEmptyState } from "../_components/EmptyState";

export const dynamic = "force-dynamic";

interface Props {
  params: Promise<{ path: string[] }>;
  searchParams: Promise<{ q?: string }>;
}

export default async function SitemapDrillPage({ params, searchParams }: Props) {
  const { path: segments } = await params;
  const { q } = await searchParams;
  const doc = await loadSitemap();

  if (!doc) {
    return (
      <section>
        <BackLink />
        <SitemapEmptyState />
      </section>
    );
  }

  const [head, ...rest] = segments;

  // /sitemap/needs-review
  if (head === "needs-review" && rest.length === 0) {
    const entries = sortEntries(filterByStatus(doc, "needs_review"));
    return (
      <DrillView title="Awaiting review" subtitle={`${entries.length} entries flagged by the Cartographer for operator review`} entries={entries} />
    );
  }

  // /sitemap/broken
  if (head === "broken" && rest.length === 0) {
    const entries = sortEntries(filterByStatus(doc, "broken"));
    return (
      <DrillView title="Broken URLs" subtitle={`${entries.length} URLs returning errors at last lint`} entries={entries} />
    );
  }

  // /sitemap/excluded
  if (head === "excluded" && rest.length === 0) {
    const entries = sortEntries(filterByStatus(doc, "excluded"));
    return (
      <DrillView title="Excluded" subtitle={`${entries.length} URLs intentionally excluded`} entries={entries} />
    );
  }

  // /sitemap/type/<type>
  if (head === "type" && rest.length === 1) {
    const type = rest[0];
    const parsed = SitemapEntryType.safeParse(type);
    if (!parsed.success) notFound();
    let entries = sortEntries(filterByType(doc, type));
    if (q) entries = searchEntries({ ...doc, entries }, q);
    return (
      <DrillView
        title={`Type · ${type}`}
        subtitle={`${entries.length} entries`}
        entries={entries}
        searchQuery={q}
        searchScope={`type/${type}`}
      />
    );
  }

  // /sitemap/status/<status>  — alternative entry point
  if (head === "status" && rest.length === 1) {
    const status = rest[0];
    const parsed = SitemapEntryStatus.safeParse(status);
    if (!parsed.success) notFound();
    const entries = sortEntries(filterByStatus(doc, parsed.data));
    return (
      <DrillView title={`Status · ${status}`} subtitle={`${entries.length} entries`} entries={entries} />
    );
  }

  // /sitemap/search?q=...
  if (head === "search") {
    const entries = q ? sortEntries(searchEntries(doc, q)) : [];
    return (
      <DrillView
        title="Search"
        subtitle={q ? `${entries.length} matches for "${q}"` : "Use the search box above"}
        entries={entries}
        searchQuery={q}
        searchScope="search"
      />
    );
  }

  notFound();
}

function BackLink() {
  return (
    <Link href="/sitemap" className="mb-4 inline-block text-xs text-primary hover:underline italic">
      &larr; back to sitemap
    </Link>
  );
}

function DrillView({
  title,
  subtitle,
  entries,
  searchQuery,
  searchScope,
}: {
  title: string;
  subtitle: string;
  entries: ReturnType<typeof sortEntries>;
  searchQuery?: string;
  searchScope?: string;
}) {
  return (
    <section className="space-y-6">
      <BackLink />
      <header className="mb-6">
        <h1 className="masthead text-3xl text-foreground">{title}</h1>
        <hr className="rule-double" />
        <p className="text-sm text-muted-foreground italic">{subtitle}</p>
      </header>

      {searchScope && (
        <form action={`/sitemap/${searchScope}`} className="flex gap-2">
          <input
            name="q"
            defaultValue={searchQuery ?? ""}
            placeholder="filter by URL, description, or contains…"
            className="flex-1 border border-border bg-card px-3 py-2 text-sm placeholder:text-muted-foreground focus:border-primary focus:outline-none"
          />
          <button
            type="submit"
            className="bg-primary px-3 py-2 text-sm font-medium text-primary-foreground hover:opacity-90"
          >
            filter
          </button>
        </form>
      )}

      {entries.length === 0 ? (
        <p className="border border-dashed border-border bg-card p-6 text-center text-sm text-muted-foreground">
          No entries match.
        </p>
      ) : (
        <div className="grid gap-3">
          {entries.map((e) => (
            <SitemapEntryCard key={e.url} entry={e} />
          ))}
        </div>
      )}
    </section>
  );
}
