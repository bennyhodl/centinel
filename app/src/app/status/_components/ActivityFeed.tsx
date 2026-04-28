import Link from "next/link";
import type { ActivityItem } from "@/lib/status";

const TYPE_TONE: Record<string, string> = {
  request: "bg-cyan-100 text-cyan-700 border-cyan-300",
  response: "bg-emerald-100 text-emerald-700 border-emerald-300",
  notify: "bg-zinc-200 text-zinc-600 border-zinc-300",
  escalation: "bg-amber-100 text-amber-700 border-amber-300",
};

function relTime(iso: string): string {
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return iso;
  const dt = Date.now() - t;
  if (dt < 60_000) return "just now";
  if (dt < 3_600_000) return `${Math.floor(dt / 60_000)}m ago`;
  if (dt < 86_400_000) return `${Math.floor(dt / 3_600_000)}h ago`;
  return `${Math.floor(dt / 86_400_000)}d ago`;
}

function refHref(ref: string): string {
  // vault paths
  if (/^vault\//i.test(ref)) return `/${ref.replace(/^vault\//i, "vault/")}`;
  if (/^Vault\//.test(ref)) return `/vault/${ref.slice("Vault/".length)}`;
  // wiki paths — send to /sitemap so the user can browse
  return `/sitemap`;
}

export default function ActivityFeed({ items }: { items: ActivityItem[] }) {
  if (items.length === 0) {
    return (
      <div className="border border-border bg-card p-4 text-sm text-muted-foreground">
        No activity in the last 7 days.
      </div>
    );
  }
  return (
    <ol className="space-y-3">
      {items.map((it) => {
        const tone =
          TYPE_TONE[it.type] ??
          "bg-secondary text-muted-foreground border-border";
        return (
          <li
            key={it.id}
            className="border border-border bg-card p-3"
          >
            <div className="flex flex-wrap items-center gap-2 text-xs">
              <span className="opacity-60">{relTime(it.timestamp)}</span>
              <span className="opacity-40">·</span>
              <span className="font-mono text-foreground/80">{it.from}</span>
              <span className="opacity-40">→</span>
              <span className="font-mono text-foreground/80">{it.to}</span>
              <span
                className={`ml-1 border px-2 py-0.5 text-[10px] uppercase tracking-wider ${tone}`}
              >
                {it.type}
              </span>
              {it.priority && it.priority !== "normal" && (
                <span className="border border-border px-2 py-0.5 text-[10px] uppercase tracking-wider text-muted-foreground">
                  {it.priority}
                </span>
              )}
            </div>
            {it.summary && (
              <p className="mt-1.5 text-sm text-foreground/80">{it.summary}</p>
            )}
            {it.references.length > 0 && (
              <div className="mt-2 flex flex-wrap gap-1.5">
                {it.references.map((r) => (
                  <Link
                    key={r}
                    href={refHref(r)}
                    className="border border-border bg-card px-2 py-0.5 font-mono text-[11px] text-muted-foreground transition hover:border-primary/40 hover:text-primary"
                  >
                    {r}
                  </Link>
                ))}
              </div>
            )}
          </li>
        );
      })}
    </ol>
  );
}
