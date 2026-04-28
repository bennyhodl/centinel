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
    <Link href="/sitemap" className="mb-4 inline-block text-xs text-tampa-cyan hover:underline">
      ← back to sitemap
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
      <header>
        <h1 className="text-2xl font-semibold">{title}</h1>
        <p className="mt-1 text-sm opacity-60">{subtitle}</p>
      </header>

      {searchScope && (
        <form action={`/sitemap/${searchScope}`} className="flex gap-2">
          <input
            name="q"
            defaultValue={searchQuery ?? ""}
            placeholder="filter by URL, description, or contains…"
            className="flex-1 rounded-md border border-white/10 bg-white/[0.02] px-3 py-2 text-sm placeholder:opacity-40 focus:border-tampa-cyan focus:outline-none"
          />
          <button
            type="submit"
            className="rounded-md bg-tampa-cyan px-3 py-2 text-sm font-medium text-tampa-ink hover:opacity-90"
          >
            filter
          </button>
        </form>
      )}

      {entries.length === 0 ? (
        <p className="rounded-md border border-dashed border-white/10 bg-white/[0.02] p-6 text-center text-sm opacity-60">
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
