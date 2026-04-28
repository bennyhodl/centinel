import Link from "next/link";
import type { ActivityItem } from "@/lib/status";

const TYPE_TONE: Record<string, string> = {
  request: "bg-cyan-500/10 text-cyan-300 border-cyan-400/30",
  response: "bg-emerald-500/10 text-emerald-300 border-emerald-400/30",
  notify: "bg-zinc-500/10 text-zinc-300 border-zinc-400/30",
  escalation: "bg-amber-500/10 text-amber-300 border-amber-400/30",
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
      <div className="rounded-lg border border-white/10 bg-white/[0.02] p-4 text-sm opacity-60">
        No activity in the last 7 days.
      </div>
    );
  }
  return (
    <ol className="space-y-3">
      {items.map((it) => {
        const tone =
          TYPE_TONE[it.type] ??
          "bg-white/5 text-white/70 border-white/20";
        return (
          <li
            key={it.id}
            className="rounded-lg border border-white/10 bg-white/[0.02] p-3"
          >
            <div className="flex flex-wrap items-center gap-2 text-xs">
              <span className="opacity-60">{relTime(it.timestamp)}</span>
              <span className="opacity-40">·</span>
              <span className="font-mono text-white/80">{it.from}</span>
              <span className="opacity-40">→</span>
              <span className="font-mono text-white/80">{it.to}</span>
              <span
                className={`ml-1 rounded-full border px-2 py-0.5 text-[10px] uppercase tracking-wider ${tone}`}
              >
                {it.type}
              </span>
              {it.priority && it.priority !== "normal" && (
                <span className="rounded-full border border-white/20 px-2 py-0.5 text-[10px] uppercase tracking-wider opacity-70">
                  {it.priority}
                </span>
              )}
            </div>
            {it.summary && (
              <p className="mt-1.5 text-sm text-white/80">{it.summary}</p>
            )}
            {it.references.length > 0 && (
              <div className="mt-2 flex flex-wrap gap-1.5">
                {it.references.map((r) => (
                  <Link
                    key={r}
                    href={refHref(r)}
                    className="rounded-md border border-white/10 bg-white/[0.03] px-2 py-0.5 font-mono text-[11px] text-white/70 transition hover:border-tampa-cyan/40 hover:text-tampa-cyan"
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
