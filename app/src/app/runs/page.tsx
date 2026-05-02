import Link from "next/link";
import { listSessions } from "@/lib/sessions";

export const dynamic = "force-dynamic";

interface PageProps {
  searchParams: Promise<{
    profile?: string;
    cronJobId?: string;
    since?: string;
  }>;
}

const PROFILE_TONE: Record<string, string> = {
  investigator: "border-cyan-400/50 bg-cyan-50 text-cyan-900",
  "watch-runner": "border-amber-400/50 bg-amber-50 text-amber-900",
  "data-reporter": "border-emerald-400/50 bg-emerald-50 text-emerald-900",
  archivist: "border-zinc-400/50 bg-zinc-50 text-zinc-900",
  default: "border-primary/50 bg-primary/10 text-primary",
};

function relTime(ms: number): string {
  const dt = Date.now() - ms;
  if (dt < 60_000) return `${Math.floor(dt / 1000)}s ago`;
  if (dt < 3_600_000) return `${Math.floor(dt / 60_000)}m ago`;
  if (dt < 86_400_000) return `${Math.floor(dt / 3_600_000)}h ago`;
  return `${Math.floor(dt / 86_400_000)}d ago`;
}

function fmtBytes(b: number): string {
  if (b < 1024) return `${b} B`;
  if (b < 1024 * 1024) return `${(b / 1024).toFixed(1)} KB`;
  return `${(b / 1024 / 1024).toFixed(1)} MB`;
}

export default async function RunsPage({ searchParams }: PageProps) {
  const sp = await searchParams;
  const profile = sp.profile && sp.profile !== "all" ? sp.profile : undefined;
  const cronJobId = sp.cronJobId;
  const sinceMs = sp.since ? Number(sp.since) : undefined;

  const items = await listSessions({
    profile: profile as never,
    cronJobId,
    sinceMs,
    limit: 200,
  });

  return (
    <section>
      <header className="mb-6 flex flex-wrap items-baseline justify-between gap-3">
        <div>
          <h1 className="masthead text-3xl text-foreground">Runs</h1>
          <hr className="rule-double" />
          <p className="text-sm text-muted-foreground italic">
            Every Hermes agent session — cron-fired and manual. Click a row to
            see the full reasoning trace, tool calls, and results.
          </p>
        </div>
        <div className="text-xs text-muted-foreground">
          <strong className="text-foreground">{items.length}</strong> sessions
        </div>
      </header>

      {/* Profile filter pills */}
      <div className="mb-4 flex flex-wrap items-center gap-2">
        <span className="font-smallcaps text-[0.6rem] tracking-[0.12em] text-muted-foreground uppercase">
          profile:
        </span>
        <FilterPill label="all" active={!profile} href={buildHref(sp, { profile: "all" })} />
        {(["investigator", "watch-runner", "data-reporter", "archivist", "default"] as const).map(
          (p) => (
            <FilterPill
              key={p}
              label={p}
              active={profile === p}
              href={buildHref(sp, { profile: p })}
              tone={PROFILE_TONE[p]}
            />
          ),
        )}
        {(profile || cronJobId || sinceMs) && (
          <Link
            href="/runs"
            className="ml-2 text-xs text-muted-foreground hover:text-primary italic"
          >
            clear filters
          </Link>
        )}
      </div>

      {cronJobId && (
        <div className="mb-3 border border-border bg-secondary/40 px-3 py-2 text-xs">
          <span className="font-smallcaps tracking-[0.12em] text-muted-foreground uppercase">
            cron_job_id:
          </span>{" "}
          <code className="font-mono text-foreground">{cronJobId}</code>
        </div>
      )}

      {items.length === 0 ? (
        <div className="border border-dashed border-border bg-card p-8 text-center text-sm text-muted-foreground">
          <p className="italic">No sessions match these filters.</p>
          <p className="mt-2 text-xs">
            New runs appear here as soon as the agent starts writing. Hit{" "}
            <strong>Run Now</strong> on any{" "}
            <Link href="/investigations" className="text-primary hover:underline">
              investigation
            </Link>{" "}
            and refresh.
          </p>
        </div>
      ) : (
        <ul className="divide-y divide-border border border-border bg-card">
          {items.map((s) => {
            const tone =
              PROFILE_TONE[s.profile] ?? "border-border bg-secondary";
            return (
              <li key={`${s.profile}/${s.id}`}>
                <Link
                  href={`/runs/${encodeURIComponent(s.id)}`}
                  className="block px-4 py-3 transition hover:bg-accent"
                >
                  <div className="flex flex-wrap items-baseline gap-2">
                    <span
                      className={`border px-1.5 py-0.5 font-smallcaps text-[0.55rem] tracking-[0.12em] uppercase ${tone}`}
                    >
                      {s.profile}
                    </span>
                    {s.cronJobId && (
                      <span className="font-smallcaps text-[0.55rem] tracking-[0.12em] text-muted-foreground uppercase">
                        cron
                      </span>
                    )}
                    <code className="font-mono text-xs text-foreground/90">
                      {s.id}
                    </code>
                    <span className="ml-auto text-[0.65rem] text-muted-foreground">
                      {relTime(s.mtimeMs)} · {fmtBytes(s.sizeBytes)}
                    </span>
                  </div>
                  {s.startedAt && (
                    <div className="mt-1 text-[0.65rem] text-muted-foreground">
                      started {new Date(s.startedAt).toLocaleString()}
                    </div>
                  )}
                </Link>
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}

function buildHref(
  sp: Record<string, string | undefined>,
  patch: Record<string, string | undefined>,
): string {
  const merged = { ...sp, ...patch };
  const params = new URLSearchParams();
  for (const [k, v] of Object.entries(merged)) {
    if (v !== undefined && v !== "all" && v !== "") params.set(k, v);
  }
  const qs = params.toString();
  return qs ? `/runs?${qs}` : "/runs";
}

function FilterPill({
  label,
  href,
  active,
  tone,
}: {
  label: string;
  href: string;
  active: boolean;
  tone?: string;
}) {
  if (active) {
    return (
      <span
        className={`border px-2 py-0.5 font-smallcaps text-[0.6rem] tracking-[0.12em] uppercase ${
          tone ?? "border-primary bg-primary/10 text-primary"
        }`}
      >
        {label}
      </span>
    );
  }
  return (
    <Link
      href={href}
      className="border border-border bg-secondary px-2 py-0.5 font-smallcaps text-[0.6rem] tracking-[0.12em] uppercase text-muted-foreground hover:border-primary/40 hover:text-primary"
    >
      {label}
    </Link>
  );
}
