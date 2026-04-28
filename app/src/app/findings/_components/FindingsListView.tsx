import Link from "next/link";
import {
  type FindingStack,
  type FindingSummary,
  listAllFindings,
  listFindings,
} from "@/lib/findings";
import { EmptyState } from "@/components/EmptyState";
import { Pill, statusTone } from "@/components/Pill";

const TABS = [
  { key: "all", label: "All" },
  { key: "raw", label: "Raw" },
  { key: "published", label: "Published" },
  { key: "draft", label: "Draft" },
] as const;

type TabKey = (typeof TABS)[number]["key"];

function isTab(s: string | undefined): s is TabKey {
  return !!s && TABS.some((t) => t.key === s);
}

export default async function FindingsListView({
  activeTab,
  showDraftBanner = false,
}: {
  activeTab: TabKey;
  showDraftBanner?: boolean;
}) {
  let items: FindingSummary[];
  if (activeTab === "all") {
    items = await listAllFindings();
  } else {
    items = await listFindings(activeTab as FindingStack);
  }

  return (
    <section>
      <header className="mb-6">
        <h1 className="masthead text-3xl text-foreground">Findings</h1>
        <hr className="rule-double" />
        <p className="text-sm text-muted-foreground italic">
          Auto-published facts (raw), operator-promoted narratives (published),
          and editor-drafted pieces (draft).
        </p>
      </header>

      <nav className="flex gap-0 border-b-2 border-foreground/20 mb-6">
        {TABS.map((t) => {
          const href =
            t.key === "draft"
              ? "/findings/draft"
              : t.key === "all"
                ? "/findings"
                : `/findings?stack=${t.key}`;
          const active = t.key === activeTab;
          return (
            <Link
              key={t.key}
              href={href}
              className={`px-4 py-2 text-sm font-smallcaps tracking-wider transition border-b-2 -mb-[2px] ${
                active
                  ? "border-primary text-primary"
                  : "border-transparent text-muted-foreground hover:text-foreground hover:border-foreground/30"
              }`}
            >
              {t.label}
            </Link>
          );
        })}
      </nav>

      {showDraftBanner && (
        <div className="border border-amber-300 bg-amber-50 p-3 text-sm text-amber-800 mb-6">
          <strong className="font-smallcaps tracking-wider text-amber-700">
            Draft &mdash;
          </strong>{" "}
          not yet reviewed by editor or counsel. Do not cite.
        </div>
      )}

      {items.length === 0 ? (
        <EmptyState title="No findings yet">
          <p>
            Findings are produced by the civic-investigator and promoted by the
            Editor.{" "}
            {activeTab === "raw"
              ? "Raw findings populate as the investigator runs."
              : activeTab === "published"
                ? "Published findings appear after operator review."
                : activeTab === "draft"
                  ? "Drafts appear when the Editor stages a narrative."
                  : "Launch one from the Editor's Desk to begin."}
          </p>
        </EmptyState>
      ) : (
        <ul className="divide-y divide-border">
          {items.map((it) => (
            <li key={`${it.stack}/${it.slug}`}>
              <Link
                href={`/findings/${it.slug}`}
                className="block py-4 px-3 transition hover:bg-accent"
              >
                <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1">
                  <Pill tone={statusTone(it.stack)}>{it.stack}</Pill>
                  <span className="font-mono text-[0.65rem] text-primary">
                    {it.slug}
                  </span>
                </div>
                <h2 className="mt-2 font-display text-lg font-semibold">
                  {(it.frontmatter.title as string | undefined) ?? it.slug}
                </h2>
                {(it.frontmatter.summary as string | undefined) && (
                  <p className="mt-1 text-sm text-foreground/80 leading-relaxed">
                    {String(it.frontmatter.summary)}
                  </p>
                )}
                {it.excerpt && !it.frontmatter.summary && (
                  <p className="mt-1 text-sm text-muted-foreground leading-relaxed">
                    {it.excerpt}
                  </p>
                )}
                <footer className="mt-2 flex flex-wrap gap-x-4 gap-y-1 text-[0.65rem] text-muted-foreground">
                  {it.frontmatter.published_at != null && (
                    <span>published {String(it.frontmatter.published_at)}</span>
                  )}
                  {it.frontmatter.drafted_at != null && (
                    <span>drafted {String(it.frontmatter.drafted_at)}</span>
                  )}
                  {it.frontmatter.generated_at != null && (
                    <span>generated {String(it.frontmatter.generated_at)}</span>
                  )}
                  {it.frontmatter.generated_by != null && (
                    <span>by {String(it.frontmatter.generated_by)}</span>
                  )}
                </footer>
              </Link>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

export { isTab, type TabKey };
