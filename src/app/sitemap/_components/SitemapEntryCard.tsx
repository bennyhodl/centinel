import type { SitemapEntry, SitemapEntryStatus } from "@/lib/sitemap";

const STATUS_STYLES: Record<SitemapEntryStatus, string> = {
  active: "bg-emerald-500/10 text-emerald-400 ring-1 ring-emerald-500/20",
  broken: "bg-red-500/10 text-red-400 ring-1 ring-red-500/20",
  excluded: "bg-zinc-500/10 text-zinc-400 ring-1 ring-zinc-500/20",
  needs_review: "bg-amber-500/10 text-amber-400 ring-1 ring-amber-500/20",
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
      className={`inline-flex items-center rounded-full px-2 py-0.5 text-[10px] font-medium uppercase tracking-wider ${STATUS_STYLES[status]}`}
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
    <article className="rounded-lg border border-white/10 bg-white/[0.02] p-4 transition hover:border-tampa-cyan/40 hover:bg-white/[0.04]">
      <div className="flex flex-wrap items-baseline gap-x-2 gap-y-1">
        <span className="rounded bg-white/5 px-1.5 py-0.5 font-mono text-[10px] uppercase tracking-wider text-tampa-cyan">
          {entry.type}
        </span>
        <StatusPill status={entry.status} />
        <span className="text-[10px] uppercase tracking-wider opacity-50">
          {entry.content_kind}
        </span>
      </div>

      <a
        href={entry.url}
        target="_blank"
        rel="noopener noreferrer"
        className="mt-2 block break-all font-mono text-sm text-white hover:text-tampa-cyan"
      >
        <span className="opacity-50">{host}</span>
        <span>{pathOnly}</span>
      </a>

      {entry.description && (
        <p className="mt-2 text-sm leading-relaxed opacity-80">
          {entry.description}
        </p>
      )}

      {entry.contains.length > 0 && (
        <ul className="mt-2 flex flex-wrap gap-1.5 text-[11px]">
          {entry.contains.map((c, i) => (
            <li
              key={i}
              className="rounded bg-white/5 px-1.5 py-0.5 opacity-70"
            >
              {c}
            </li>
          ))}
        </ul>
      )}

      <footer className="mt-3 flex flex-wrap gap-x-4 gap-y-1 text-[11px] opacity-50">
        <span>last crawled {entry.last_crawled}</span>
        {entry.crawl_freq && <span>freq: {entry.crawl_freq}</span>}
        {entry.parser && <span>parser: {entry.parser}</span>}
      </footer>
    </article>
  );
}
