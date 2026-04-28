import Link from "next/link";
import { listEntityTypes } from "@/lib/entities";
import { EmptyState } from "@/components/EmptyState";

export const dynamic = "force-dynamic";

export default async function EntitiesPage() {
  const types = await listEntityTypes();
  const total = types.reduce((s, t) => s + t.count, 0);

  return (
    <section className="space-y-6">
      <header>
        <h1 className="text-2xl font-semibold">Entities</h1>
        <p className="mt-1 text-sm opacity-60">
          Contractors, people, organizations, and projects extracted from
          findings.
        </p>
      </header>

      {total === 0 ? (
        <EmptyState title="No entities yet">
          <p>
            Entities are extracted automatically from findings as the
            civic-investigator runs. They populate{" "}
            <code className="font-mono text-tampa-cyan">
              &lt;wiki&gt;/Entities/
            </code>
            . Launch an investigation from{" "}
            <Link href="/chat" className="text-tampa-cyan hover:underline">
              /chat
            </Link>{" "}
            to start.
          </p>
        </EmptyState>
      ) : (
        <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
          {types.map((t) => (
            <Link
              key={t.type}
              href={`/entities/${t.type}`}
              className="rounded-lg border border-white/10 bg-white/[0.02] p-4 transition hover:border-tampa-cyan/40 hover:bg-white/[0.04]"
            >
              <div className="text-xs uppercase tracking-wider opacity-60">
                {t.label}
              </div>
              <div className="mt-1 font-mono text-2xl font-semibold">
                {t.count}
              </div>
            </Link>
          ))}
        </div>
      )}
    </section>
  );
}
