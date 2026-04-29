import Link from "next/link";
import { notFound } from "next/navigation";
import { findFindingAcrossStacks } from "@/lib/findings";
import MarkdownView from "@/components/MarkdownView";
import { Pill, statusTone } from "@/components/Pill";
import { PromoteButton } from "../_components/PromoteButton";

export const dynamic = "force-dynamic";

function isExternalUrl(s: string): boolean {
  return /^https?:\/\//i.test(s);
}

function vaultHref(p: string): string {
  // strip leading "Vault/" if present, since /vault is rooted there
  const cleaned = p.replace(/^\/+/, "").replace(/^Vault\//, "");
  return `/vault/${cleaned}`;
}

export default async function FindingDetailPage({
  params,
}: {
  params: Promise<{ slug: string }>;
}) {
  const { slug } = await params;
  const doc = await findFindingAcrossStacks(slug);
  if (!doc) notFound();

  const fm = doc.frontmatter;
  const summary = fm.summary as string | undefined;
  const isDraft = doc.stack === "draft";

  return (
    <section className="space-y-6">
      <header className="mb-6">
        <div className="text-xs text-muted-foreground">
          <Link href="/findings" className="hover:text-primary italic">
            &larr; Findings
          </Link>
        </div>
        <div className="mt-2 flex flex-wrap items-center gap-3">
          <Pill tone={statusTone(doc.stack)}>{doc.stack}</Pill>
          <span className="font-mono text-[0.65rem] text-primary">{slug}</span>
        </div>
        <h1 className="mt-2 masthead text-3xl text-foreground">
          {(fm.title as string | undefined) ?? slug}
        </h1>
        <hr className="rule-double" />
        {summary && <p className="text-base text-foreground/80 leading-relaxed">{summary}</p>}
      </header>

      {isDraft && (
        <div className="space-y-3 border border-amber-300 bg-amber-50 p-3 text-sm text-amber-800">
          <div>
            <strong className="font-smallcaps tracking-wider text-amber-700">
              Draft &mdash;
            </strong>{" "}
            not yet reviewed by editor or counsel. Do not cite.
          </div>
          <PromoteButton slug={slug} />
        </div>
      )}

      <article className="border border-border bg-card p-5">
        <MarkdownView source={doc.body} />
      </article>

      <section>
        <h2 className="mb-3 section-header">
          Sources
        </h2>
        {doc.sources.length === 0 ? (
          <p className="text-sm text-muted-foreground">No sources recorded.</p>
        ) : (
          <ul className="space-y-2">
            {doc.sources.map((s, i) => {
              if (isExternalUrl(s)) {
                return (
                  <li key={i}>
                    <a
                      href={s}
                      target="_blank"
                      rel="noopener noreferrer"
                      className="break-all font-mono text-sm text-primary hover:underline"
                    >
                      {s}
                    </a>
                  </li>
                );
              }
              return (
                <li key={i}>
                  <Link
                    href={vaultHref(s)}
                    className="break-all font-mono text-sm text-primary hover:underline"
                  >
                    {s}
                  </Link>
                </li>
              );
            })}
          </ul>
        )}
      </section>

      <footer className="flex flex-wrap gap-x-4 gap-y-1 border-t-2 border-foreground/15 pt-4 text-[0.65rem] text-muted-foreground">
        {fm.generated_at != null && (
          <span>generated {String(fm.generated_at)}</span>
        )}
        {fm.generated_by != null && <span>by {String(fm.generated_by)}</span>}
        {fm.drafted_at != null && <span>drafted {String(fm.drafted_at)}</span>}
        {fm.published_at != null && (
          <span>published {String(fm.published_at)}</span>
        )}
      </footer>
    </section>
  );
}
