import type { InvestigationCronStatus } from "@/lib/investigation-cron";
import { LocalTime } from "@/components/local-time";

export function CronStatusCard({ status }: { status: InvestigationCronStatus }) {
  // Manual schedule is intentional, not a problem.
  const isManual = status.schedule_word === "manual";

  if (isManual) {
    return (
      <div className="border border-border bg-card p-3">
        <div className="font-smallcaps text-[0.55rem] tracking-[0.12em] text-muted-foreground">
          Cron
        </div>
        <div className="mt-1 text-sm">
          <span className="font-mono text-muted-foreground">manual</span>
          <span className="ml-2 text-xs italic text-muted-foreground">
            operator triggers only
          </span>
        </div>
      </div>
    );
  }

  if (!status.registered) {
    return (
      <div className="border border-amber-300 bg-amber-50 p-3">
        <div className="font-smallcaps text-[0.55rem] tracking-[0.12em] text-amber-700">
          Cron — not registered
        </div>
        <div className="mt-1 text-xs text-amber-900 whitespace-pre-wrap">
          {status.error ?? "No cron job found in the investigator profile."}
        </div>
        <div className="mt-2 font-mono text-[0.65rem] text-amber-900/80">
          Run on host: <code>bin/centinel investigate register {status.slug}</code>
        </div>
      </div>
    );
  }

  const dotClass = status.active
    ? "bg-emerald-500"
    : status.active === false
      ? "bg-amber-400"
      : "bg-muted-foreground";
  const stateLabel = status.active
    ? "active"
    : status.active === false
      ? "paused"
      : "unknown";

  return (
    <div className="border border-border bg-card p-3">
      <div className="font-smallcaps text-[0.55rem] tracking-[0.12em] text-muted-foreground">
        Cron
      </div>
      <div className="mt-1 flex items-center gap-2 text-sm">
        <span className={`inline-flex h-2 w-2 rounded-full ${dotClass}`} />
        <span className="font-mono">{stateLabel}</span>
        {status.job_id && (
          <code className="ml-1 text-[0.65rem] text-muted-foreground">
            {status.job_id}
          </code>
        )}
      </div>
      <dl className="mt-2 grid grid-cols-1 gap-y-0.5 text-[0.7rem] text-muted-foreground">
        {status.schedule_cron && (
          <Row label="Schedule" value={status.schedule_cron} />
        )}
        {status.next_run && (
          <Row
            label="Next run"
            value={<LocalTime iso={status.next_run} showRelative />}
          />
        )}
        {status.last_run && (
          <Row
            label="Last run"
            value={<LocalTime iso={status.last_run} showRelative />}
          />
        )}
      </dl>
    </div>
  );
}

function Row({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <div className="flex justify-between gap-2">
      <dt className="shrink-0">{label}</dt>
      <dd className="truncate font-mono text-foreground/80">{value}</dd>
    </div>
  );
}
