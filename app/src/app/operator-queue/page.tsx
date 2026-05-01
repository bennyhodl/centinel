import {
  AGE_AMBER_THRESHOLD_MS,
  formatAge,
  listQueueGrouped,
} from "@/lib/operator-queue";
import { EmptyState } from "@/components/EmptyState";
import { Pill, statusTone } from "@/components/Pill";
import { ResolveButtons } from "./_components/ResolveButtons";
import Link from "next/link";

export const dynamic = "force-dynamic";

export default async function OperatorQueuePage() {
  const groups = await listQueueGrouped();
  const total = groups.reduce((s, g) => s + g.items.length, 0);

  return (
    <section className="space-y-6">
      <header className="mb-6">
        <h1 className="masthead text-3xl text-foreground">Operator Queue</h1>
        <hr className="rule-double" />
        <p className="text-sm text-muted-foreground italic">
          Items awaiting human judgment. Resolve inline — bookkeeping
          decisions apply immediately; agent-required actions queue an
          inbox directive for the next cron tick.
        </p>
      </header>

      {total === 0 ? (
        <EmptyState title="Queue is empty">
          <p className="mb-3">
            Nothing is waiting on your decision. Items show up here when
            agents flag entity merges, watch tuning, aging drafts, or broken
            watches — i.e. only after they&apos;ve actually run and found
            something ambiguous.
          </p>
          <p className="text-xs text-muted-foreground">
            If you just installed Centinel and the queue stays empty, it
            usually means the agents haven&apos;t had enough material to
            disagree about yet. Create an{" "}
            <Link href="/investigations" className="text-primary hover:underline">
              investigation
            </Link>{" "}
            or check the{" "}
            <Link href="/status" className="text-primary hover:underline">
              status page
            </Link>{" "}
            to see what&apos;s currently running.
          </p>
        </EmptyState>
      ) : (
        <div className="space-y-8">
          {groups.map((g) => (
            <div key={g.bucket}>
              <div className="mb-3 flex items-baseline gap-2">
                <h2 className="section-header">
                  {g.label}
                </h2>
                <span className="font-mono text-xs text-muted-foreground">
                  {g.items.length}
                </span>
              </div>
              {g.items.length === 0 ? (
                <p className="text-xs text-muted-foreground">empty</p>
              ) : (
                <ul className="grid gap-3">
                  {g.items.map((it) => {
                    const aging =
                      it.ageMs != null && it.ageMs > AGE_AMBER_THRESHOLD_MS;
                    return (
                      <li
                        key={`${g.bucket}/${it.slug}`}
                        className={`border bg-card p-4 ${
                          aging
                            ? "border-amber-400"
                            : "border-border"
                        }`}
                      >
                        <div className="flex flex-wrap items-baseline gap-x-2 gap-y-1">
                          <span className="rounded bg-secondary px-1.5 py-0.5 font-mono text-[10px] uppercase tracking-wider text-primary">
                            {g.bucket}
                          </span>
                          <Pill tone={statusTone(it.status)}>{it.status}</Pill>
                          <span
                            className={`text-[11px] ${
                              aging ? "text-amber-600" : "opacity-50"
                            }`}
                          >
                            {formatAge(it.ageMs)}
                          </span>
                        </div>
                        <h3 className="mt-2 text-sm font-semibold">
                          {it.title}
                        </h3>
                        {it.excerpt && (
                          <p className="mt-1 text-sm text-foreground/80">
                            {it.excerpt}
                          </p>
                        )}
                        <div className="mt-2 font-mono text-[11px] text-muted-foreground">
                          {it.slug}
                        </div>
                        <div className="mt-3">
                          <ResolveButtons
                            bucket={g.bucket}
                            slug={it.slug}
                            status={it.status}
                          />
                        </div>
                      </li>
                    );
                  })}
                </ul>
              )}
            </div>
          ))}
        </div>
      )}
    </section>
  );
}
