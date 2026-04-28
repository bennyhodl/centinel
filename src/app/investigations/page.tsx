import Link from "next/link";
import { listInvestigations } from "@/lib/investigations";
import { EmptyState } from "@/components/EmptyState";
import { Pill, statusTone } from "@/components/Pill";

export const dynamic = "force-dynamic";

export default async function InvestigationsPage() {
  const items = await listInvestigations();

  return (
    <section className="space-y-6">
      <header className="flex flex-wrap items-baseline justify-between gap-3">
        <div>
          <h1 className="text-2xl font-semibold">Investigations</h1>
          <p className="mt-1 text-sm opacity-60">
            Persistent civic-investigator sessions, each with a slug, schedule,
            and seed set.
          </p>
        </div>
        <Link
          href="/chat"
          className="rounded-md border border-tampa-cyan/40 bg-tampa-cyan/10 px-3 py-1.5 text-sm text-tampa-cyan transition hover:bg-tampa-cyan/15"
          title="Editor registers new investigations from the chat surface."
        >
          + New investigation
        </Link>
      </header>

      {items.length === 0 ? (
        <EmptyState title="No investigations yet">
          <p>
            Investigations are persistent sessions registered by the Editor.
            Launch one from{" "}
            <Link href="/chat" className="text-tampa-cyan hover:underline">
              /chat
            </Link>{" "}
            or via:
          </p>
          <pre className="mx-auto mt-4 overflow-auto rounded bg-black/40 p-3 text-left font-mono text-xs text-tampa-cyan">
            hermes session run civic-investigator
          </pre>
        </EmptyState>
      ) : (
        <ul className="grid gap-3">
          {items.map((it) => {
            const fm = it.frontmatter;
            const seeds = Array.isArray(fm.seeds) ? fm.seeds.length : 0;
            const status = (fm.status as string | undefined) ?? "active";
            return (
              <li key={it.slug}>
                <Link
                  href={`/investigations/${it.slug}`}
                  className="block rounded-lg border border-white/10 bg-white/[0.02] p-4 transition hover:border-tampa-cyan/40 hover:bg-white/[0.04]"
                >
                  <div className="flex flex-wrap items-baseline gap-x-2 gap-y-1">
                    <Pill tone={statusTone(status)}>{status}</Pill>
                    <span className="font-mono text-[11px] text-tampa-cyan">
                      {it.slug}
                    </span>
                  </div>
                  <h2 className="mt-2 text-base font-semibold">
                    {(fm.title as string | undefined) ?? it.slug}
                  </h2>
                  {it.excerpt && (
                    <p className="mt-1 text-sm opacity-75">{it.excerpt}</p>
                  )}
                  <footer className="mt-3 flex flex-wrap gap-x-4 gap-y-1 text-[11px] opacity-50">
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
