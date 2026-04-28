import {
  AGE_AMBER_THRESHOLD_MS,
  formatAge,
  listQueueGrouped,
} from "@/lib/operator-queue";
import { EmptyState } from "@/components/EmptyState";
import { Pill, statusTone } from "@/components/Pill";

export const dynamic = "force-dynamic";

export default async function OperatorQueuePage() {
  const groups = await listQueueGrouped();
  const total = groups.reduce((s, g) => s + g.items.length, 0);

  return (
    <section className="space-y-6">
      <header>
        <h1 className="text-2xl font-semibold">Operator queue</h1>
        <p className="mt-1 text-sm opacity-60">
          Items awaiting human judgment. Resolutions happen via{" "}
          <code className="font-mono text-tampa-cyan">/chat</code>; this surface
          is read-only.
        </p>
      </header>

      {total === 0 ? (
        <EmptyState title="Queue is empty">
          <p>
            Nothing is waiting on the operator. Items show up here when agents
            flag entity merges, watch tuning, aging drafts, or broken watches.
          </p>
        </EmptyState>
      ) : (
        <div className="space-y-8">
          {groups.map((g) => (
            <div key={g.bucket}>
              <div className="mb-3 flex items-baseline gap-2">
                <h2 className="text-sm font-semibold uppercase tracking-wider opacity-60">
                  {g.label}
                </h2>
                <span className="font-mono text-xs opacity-50">
                  {g.items.length}
                </span>
              </div>
              {g.items.length === 0 ? (
                <p className="text-xs opacity-40">empty</p>
              ) : (
                <ul className="grid gap-3">
                  {g.items.map((it) => {
                    const aging =
                      it.ageMs != null && it.ageMs > AGE_AMBER_THRESHOLD_MS;
                    return (
                      <li
                        key={`${g.bucket}/${it.slug}`}
                        className={`rounded-lg border bg-white/[0.02] p-4 ${
                          aging
                            ? "border-amber-500/40"
                            : "border-white/10"
                        }`}
                      >
                        <div className="flex flex-wrap items-baseline gap-x-2 gap-y-1">
                          <span className="rounded bg-white/5 px-1.5 py-0.5 font-mono text-[10px] uppercase tracking-wider text-tampa-cyan">
                            {g.bucket}
                          </span>
                          <Pill tone={statusTone(it.status)}>{it.status}</Pill>
                          <span
                            className={`text-[11px] ${
                              aging ? "text-amber-400" : "opacity-50"
                            }`}
                          >
                            {formatAge(it.ageMs)}
                          </span>
                        </div>
                        <h3 className="mt-2 text-sm font-semibold">
                          {it.title}
                        </h3>
                        {it.excerpt && (
                          <p className="mt-1 text-sm opacity-75">
                            {it.excerpt}
                          </p>
                        )}
                        <div className="mt-2 font-mono text-[11px] opacity-40">
                          {it.slug}
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
