import { listInvestigations } from "@/lib/investigations";
import type { SitemapEntry, SitemapLink } from "@/lib/sitemap";
import { StatusPill } from "../../../../_components/SitemapEntryCard";
import { LeafActions } from "./LeafActions";
import { LinkTable } from "./LinkTable";

export async function LeafDetail({
  entry,
  sitemapUrls,
}: {
  entry: SitemapEntry;
  sitemapUrls: Set<string>;
}) {
  // Hydrate link kinds: mark links that point to URLs in our sitemap.
  const links: SitemapLink[] = (entry.links ?? []).map((l) => {
    if (sitemapUrls.has(l.href)) return { ...l, kind: "sitemap" };
    return l;
  });

  // Backlinks: any sitemap entry whose `links[].href` matches this entry's URL
  // We don't have the full doc here; backlinks are computed in the page and
  // could be passed in — for now we leave that as a TODO and skip.
  // (Future: pass `backlinks: SitemapEntry[]` from the page.)

  let host = "";
  let pathOnly = entry.url;
  try {
    const u = new URL(entry.url);
    host = u.host;
    pathOnly = u.pathname + (u.search || "");
  } catch {}

  // Investigation list for the seed dropdown
  let investigations: { slug: string; title: string }[] = [];
  try {
    const list = await listInvestigations();
    investigations = list.map((i) => ({
      slug: i.slug,
      title: String(i.frontmatter?.title ?? i.slug),
    }));
  } catch {
    investigations = [];
  }

  return (
    <article className="border border-border bg-card p-5">
      {/* Header row: type / status / kind / score */}
      <div className="flex flex-wrap items-baseline gap-x-2 gap-y-1">
        <span className="border border-primary/30 bg-primary/5 px-1.5 py-0.5 font-smallcaps text-[0.55rem] tracking-[0.12em] text-primary uppercase">
          {entry.type}
        </span>
        <StatusPill status={entry.status} />
        <span className="font-smallcaps text-[0.55rem] tracking-[0.12em] text-muted-foreground">
          {entry.content_kind}
        </span>
        {typeof entry.signal_score === "number" && (
          <span className="font-smallcaps text-[0.55rem] tracking-[0.12em] text-foreground/70">
            signal {"★".repeat(entry.signal_score)}
            {"☆".repeat(3 - entry.signal_score)}
          </span>
        )}
      </div>

      {/* URL */}
      <a
        href={entry.url}
        target="_blank"
        rel="noopener noreferrer"
        className="mt-3 block break-all font-mono text-sm text-foreground hover:text-primary transition-colors"
      >
        <span className="text-muted-foreground">{host}</span>
        <span>{pathOnly}</span>{" "}
        <span className="text-muted-foreground/60 text-xs">↗</span>
      </a>

      {/* Description */}
      {entry.description && (
        <p className="mt-3 text-sm leading-relaxed text-foreground/85">
          {entry.description}
        </p>
      )}

      {/* Contains tags */}
      {entry.contains.length > 0 && (
        <ul className="mt-3 flex flex-wrap gap-1.5 text-[0.65rem]">
          {entry.contains.map((c, i) => (
            <li
              key={i}
              className="border border-border bg-secondary px-1.5 py-0.5 text-muted-foreground"
            >
              {c}
            </li>
          ))}
        </ul>
      )}

      {/* Notes */}
      {entry.notes.length > 0 && (
        <ul className="mt-3 list-disc pl-5 space-y-1 text-xs text-foreground/70">
          {entry.notes.map((n, i) => (
            <li key={i}>{n}</li>
          ))}
        </ul>
      )}

      {/* Action bar */}
      <div className="mt-5 border-t border-border pt-4">
        <LeafActions
          entry={entry}
          investigations={investigations}
        />
      </div>

      {/* Outgoing links */}
      <div className="mt-5">
        <div className="section-header text-xs">
          Outgoing links {links.length > 0 && <span className="text-muted-foreground">· {links.length}</span>}
        </div>
        {links.length === 0 ? (
          <p className="border border-dashed border-border bg-card/50 p-4 text-xs text-muted-foreground italic">
            No links recorded yet. Run the Cartographer&apos;s{" "}
            <code>enrich</code> mode to backfill outgoing links from this page.
          </p>
        ) : (
          <LinkTable links={links} pageUrl={entry.url} />
        )}
      </div>

      {/* Investigation refs */}
      {entry.investigation_refs && entry.investigation_refs.length > 0 && (
        <div className="mt-5">
          <div className="section-header text-xs">Seeded into</div>
          <ul className="flex flex-wrap gap-2 text-xs">
            {entry.investigation_refs.map((slug) => (
              <li
                key={slug}
                className="border border-primary/30 bg-primary/5 px-2 py-0.5 text-primary"
              >
                <a href={`/investigations/${slug}`} className="hover:underline">
                  {slug}
                </a>
              </li>
            ))}
          </ul>
        </div>
      )}

      {/* Footer */}
      <footer className="mt-5 flex flex-wrap gap-x-4 gap-y-1 text-[0.65rem] text-muted-foreground border-t border-border pt-3">
        <span>last crawled {entry.last_crawled}</span>
        {entry.crawl_freq && <span>freq: {entry.crawl_freq}</span>}
        {entry.parser && <span>parser: {entry.parser}</span>}
      </footer>
    </article>
  );
}
