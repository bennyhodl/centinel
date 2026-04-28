import Link from "next/link";
import { listEntityTypes } from "@/lib/entities";
import { EmptyState } from "@/components/EmptyState";

export const dynamic = "force-dynamic";

export default async function EntitiesPage() {
  const types = await listEntityTypes();
  const total = types.reduce((s, t) => s + t.count, 0);

  return (
    <section>
      <header className="mb-6">
        <h1 className="masthead text-3xl text-foreground">
          Entities &amp; Persons
        </h1>
        <hr className="rule-double" />
        <p className="text-sm text-muted-foreground italic">
          Contractors, people, organizations, and projects extracted from
          findings.
        </p>
      </header>

      {total === 0 ? (
        <EmptyState title="No entities yet">
          <p>
            Entities are extracted automatically from findings as the
            civic-investigator runs. They populate{" "}
            <code className="font-mono text-primary text-xs">
              &lt;wiki&gt;/Entities/
            </code>
            . Launch an investigation from{" "}
            <Link href="/chat" className="text-primary hover:underline">
              the Editor&apos;s Desk
            </Link>{" "}
            to start.
          </p>
        </EmptyState>
      ) : (
        <div className="grid grid-cols-2 gap-4 sm:grid-cols-4">
          {types.map((t) => (
            <Link
              key={t.type}
              href={`/entities/${t.type}`}
              className="border border-border bg-card p-5 text-center transition hover:bg-accent group"
            >
              <div className="font-smallcaps text-[0.65rem] tracking-[0.15em] text-muted-foreground uppercase">
                {t.label}
              </div>
              <div className="mt-2 font-display text-3xl font-bold text-foreground group-hover:text-primary transition-colors">
                {t.count}
              </div>
            </Link>
          ))}
        </div>
      )}
    </section>
  );
}
