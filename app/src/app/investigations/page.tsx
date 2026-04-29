import Link from "next/link";
import { listInvestigations } from "@/lib/investigations";
import { EmptyState } from "@/components/EmptyState";
import { Pill, statusTone } from "@/components/Pill";
import { NewInvestigationForm } from "./_components/NewInvestigationForm";

export const dynamic = "force-dynamic";

export default async function InvestigationsPage() {
  const items = await listInvestigations();

  return (
    <section>
      <header className="mb-6">
        <h1 className="masthead text-3xl text-foreground">Investigations</h1>
        <hr className="rule-double" />
        <p className="text-sm text-muted-foreground italic">
          Persistent civic-investigator sessions, each with a slug, schedule,
          and seed set.
        </p>
      </header>

      <NewInvestigationForm />

      {items.length === 0 ? (
        <EmptyState title="No investigations yet">
          <p>
            Click <strong>+ New Investigation</strong> above to launch your
            first one. Each investigation is a markdown file in{" "}
            <code>Investigations/</code> plus a registered cron job — the
            Investigator agent picks it up on its next tick.
          </p>
        </EmptyState>
      ) : (
        <ul className="divide-y divide-border">
          {items.map((it) => {
            const fm = it.frontmatter;
            const seeds = Array.isArray(fm.seeds) ? fm.seeds.length : 0;
            const status = (fm.status as string | undefined) ?? "active";
            return (
              <li key={it.slug}>
                <Link
                  href={`/investigations/${it.slug}`}
                  className="block py-4 px-3 transition hover:bg-accent"
                >
                  <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1">
                    <Pill tone={statusTone(status)}>{status}</Pill>
                    <span className="font-mono text-[0.65rem] text-primary">
                      {it.slug}
                    </span>
                  </div>
                  <h2 className="mt-2 font-display text-lg font-semibold">
                    {(fm.title as string | undefined) ?? it.slug}
                  </h2>
                  {it.excerpt && (
                    <p className="mt-1 text-sm text-muted-foreground leading-relaxed">
                      {it.excerpt}
                    </p>
                  )}
                  <footer className="mt-2 flex flex-wrap gap-x-4 gap-y-1 text-[0.65rem] text-muted-foreground">
                    <span>seeds: {seeds}</span>
                    {fm.schedule != null && (
                      <span>schedule: {String(fm.schedule)}</span>
                    )}
                    {fm.depth != null && <span>depth: {String(fm.depth)}</span>}
                    {fm.last_run != null && (
                      <span>last run: {String(fm.last_run)}</span>
                    )}
                  </footer>
                </Link>
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}
