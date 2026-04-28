import { listRecentActivity, readBoard } from "@/lib/status";
import LiveBoard from "./_components/LiveBoard";
import ActivityFeed from "./_components/ActivityFeed";

export const dynamic = "force-dynamic";

export default async function StatusPage() {
  const [board, activity] = await Promise.all([
    readBoard(),
    listRecentActivity(7),
  ]);

  const empty = !board.body.trim() && activity.length === 0;

  return (
    <section className="space-y-6">
      <header>
        <h1 className="text-2xl font-semibold">Status board</h1>
        <p className="mt-1 text-sm opacity-60">
          What every agent is doing — live. Updates within seconds of any
          edit to{" "}
          <code className="font-mono text-xs">
            _runtime/status/board.md
          </code>
          .
        </p>
      </header>

      {empty ? (
        <div className="rounded-lg border border-white/10 bg-white/[0.02] p-6 text-sm opacity-70">
          No agent activity yet. After setup completes and cron activates,
          this page shows what every agent is doing in real time.
        </div>
      ) : (
        <div className="grid gap-8 lg:grid-cols-[minmax(0,2fr)_minmax(0,1fr)]">
          <div>
            <LiveBoard
              initialMarkdown={board.body}
              initialMtime={board.mtime}
            />
          </div>
          <aside>
            <h2 className="mb-3 text-sm font-semibold uppercase tracking-wider opacity-60">
              7-day activity
            </h2>
            <ActivityFeed items={activity} />
          </aside>
        </div>
      )}
    </section>
  );
}
