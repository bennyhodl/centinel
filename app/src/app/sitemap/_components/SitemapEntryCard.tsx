import type { SitemapEntry, SitemapEntryStatus } from "@/lib/sitemap";

const STATUS_STYLES: Record<SitemapEntryStatus, string> = {
  active: "border-emerald-700/40 text-emerald-800 bg-emerald-50",
  broken: "border-red-700/40 text-red-800 bg-red-50",
  excluded: "border-foreground/20 text-muted-foreground bg-secondary",
  needs_review: "border-amber-700/40 text-amber-800 bg-amber-50",
};

const STATUS_LABEL: Record<SitemapEntryStatus, string> = {
  active: "active",
  broken: "broken",
  excluded: "excluded",
  needs_review: "needs review",
};

export function StatusPill({ status }: { status: SitemapEntryStatus }) {
  return (
    <span
      className={`inline-flex items-center border px-2 py-0.5 font-smallcaps text-[0.6rem] tracking-[0.12em] uppercase ${STATUS_STYLES[status]}`}
    >
      {STATUS_LABEL[status]}
    </span>
  );
}

export function SitemapEntryCard({ entry }: { entry: SitemapEntry }) {
  let host = "";
  let pathOnly = entry.url;
  try {
    const u = new URL(entry.url);
    host = u.host;
    pathOnly = u.pathname + (u.search || "");
  } catch {
    // keep raw URL
  }

  return (
    <article className="border border-border bg-card p-4 transition hover:bg-accent">
      <div className="flex flex-wrap items-baseline gap-x-2 gap-y-1">
        <span className="border border-primary/30 bg-primary/5 px-1.5 py-0.5 font-smallcaps text-[0.55rem] tracking-[0.12em] text-primary uppercase">
          {entry.type}
        </span>
        <StatusPill status={entry.status} />
        <span className="font-smallcaps text-[0.55rem] tracking-[0.12em] text-muted-foreground">
          {entry.content_kind}
        </span>
      </div>

      <a
        href={entry.url}
        target="_blank"
        rel="noopener noreferrer"
        className="mt-2 block break-all font-mono text-sm text-foreground hover:text-primary transition-colors"
      >
        <span className="text-muted-foreground">{host}</span>
        <span>{pathOnly}</span>
      </a>

      {entry.description && (
        <p className="mt-2 text-sm leading-relaxed text-foreground/80">
          {entry.description}
        </p>
      )}

      {entry.contains.length > 0 && (
        <ul className="mt-2 flex flex-wrap gap-1.5 text-[0.65rem]">
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

      <footer className="mt-3 flex flex-wrap gap-x-4 gap-y-1 text-[0.65rem] text-muted-foreground">
        <span>last crawled {entry.last_crawled}</span>
        {entry.crawl_freq && <span>freq: {entry.crawl_freq}</span>}
        {entry.parser && <span>parser: {entry.parser}</span>}
      </footer>
    </article>
  );
}
