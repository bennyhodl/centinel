import Link from "next/link";
import { listInvestigations } from "@/lib/investigations";
import { EmptyState } from "@/components/EmptyState";
import { Pill, statusTone } from "@/components/Pill";

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

      <div className="flex justify-end mb-4">
        <Link
          href="/chat"
          className="border border-primary/40 bg-primary/5 px-3 py-1.5 text-sm text-primary transition hover:bg-primary/10 font-smallcaps tracking-wider"
          title="Editor registers new investigations from the chat surface."
        >
          + New Investigation
        </Link>
      </div>

      {items.length === 0 ? (
        <EmptyState title="No investigations yet">
          <p>
            Investigations are persistent sessions registered by the Editor.
            Launch one from{" "}
            <Link href="/chat" className="text-primary hover:underline">
              the Editor&apos;s Desk
            </Link>{" "}
            or via:
          </p>
          <pre className="mx-auto mt-4 overflow-auto border border-border bg-secondary p-3 text-left font-mono text-xs text-foreground/80">
            hermes session run civic-investigator
          </pre>
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
