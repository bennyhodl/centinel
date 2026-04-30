import Link from "next/link";
import { notFound } from "next/navigation";
import { extractFindingLinks, readInvestigation } from "@/lib/investigations";
import { readInvestigationCronStatus } from "@/lib/investigation-cron";
import MarkdownView from "@/components/MarkdownView";
import { Pill, statusTone } from "@/components/Pill";
import { resolveWikilink } from "@/lib/wiki";
import { InvestigationControls } from "./_components/InvestigationControls";
import { CronStatusCard } from "./_components/CronStatusCard";
import { LocalTime } from "@/components/local-time";

export const dynamic = "force-dynamic";

export default async function InvestigationDetailPage({
  params,
}: {
  params: Promise<{ slug: string }>;
}) {
  const { slug } = await params;
  const doc = await readInvestigation(slug);
  if (!doc) notFound();

  const fm = doc.frontmatter;
  const status = (fm.status as string | undefined) ?? "active";
  const seeds = Array.isArray(fm.seeds) ? (fm.seeds as unknown[]) : [];
  const findingLinks = extractFindingLinks(doc.body);
  const cronStatus = await readInvestigationCronStatus(slug);
  // Prefer cron's last_run (authoritative) over frontmatter (which may be stale).
  const lastRunIso = cronStatus.last_run ?? (fm.last_run ? String(fm.last_run) : null);

  return (
    <section>
      <header className="mb-6">
        <div className="text-xs text-muted-foreground">
          <Link href="/investigations" className="hover:text-primary italic">
            &larr; Investigations
          </Link>
        </div>
        <h1 className="mt-2 masthead text-3xl text-foreground">
          {(fm.title as string | undefined) ?? slug}
        </h1>
        <hr className="rule-double" />
        <div className="flex flex-wrap items-center gap-3">
          <Pill tone={statusTone(status)}>{status}</Pill>
          <span className="font-mono text-[0.65rem] text-primary">{slug}</span>
        </div>
        <div className="mt-3">
          <InvestigationControls slug={slug} status={status} />
        </div>
      </header>

      {status === "paused" && (
        <div className="border border-foreground/20 bg-secondary p-3 text-sm mb-6">
          This investigation is <strong>paused</strong>. No scheduled runs will
          fire until it is resumed.
        </div>
      )}

      {/* Config strip */}
      <dl className="grid grid-cols-2 gap-3 sm:grid-cols-4 mb-6">
        <Stat label="Schedule" value={fm.schedule ? String(fm.schedule) : "—"} />
        <Stat label="Depth" value={fm.depth != null ? String(fm.depth) : "—"} />
        <Stat label="Seeds" value={String(seeds.length)} />
        <Stat label="Last Run" value={lastRunIso ? <LocalTime iso={lastRunIso} showRelative /> : "—"} />
      </dl>

      <div className="mb-6">
        <CronStatusCard status={cronStatus} />
      </div>

      <div className="grid gap-6 md:grid-cols-[1fr_240px]">
        <article className="border border-border bg-card p-6">
          <MarkdownView source={doc.body} />
        </article>

        <aside className="space-y-4">
          {seeds.length > 0 && (
            <div className="border border-border bg-card p-4">
              <div className="section-header">Seeds</div>
              <ul className="space-y-1 break-all font-mono text-[0.65rem]">
                {seeds.map((s, i) => (
                  <li key={i} className="text-foreground/80">
                    {String(s)}
                  </li>
                ))}
              </ul>
            </div>
          )}

          <div className="border border-border bg-card p-4">
            <div className="section-header">Linked Findings</div>
            {findingLinks.length === 0 ? (
              <p className="text-xs text-muted-foreground italic">
                No [[Findings/…]] wikilinks found in body.
              </p>
            ) : (
              <ul className="space-y-1 text-sm">
                {findingLinks.map((t) => (
                  <li key={t}>
                    <Link
                      href={resolveWikilink(t)}
                      className="text-primary hover:underline"
                    >
                      {t}
                    </Link>
                  </li>
                ))}
              </ul>
            )}
          </div>
        </aside>
      </div>
    </section>
  );
}

function Stat({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <div className="border border-border bg-card px-3 py-2">
      <div className="font-smallcaps text-[0.55rem] tracking-[0.12em] text-muted-foreground">
        {label}
      </div>
      <div className="mt-0.5 truncate font-mono text-sm">{value}</div>
    </div>
  );
}
