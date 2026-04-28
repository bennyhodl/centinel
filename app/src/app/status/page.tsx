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
    <section>
      <header className="mb-6">
        <h1 className="masthead text-3xl text-foreground">Status Board</h1>
        <hr className="rule-double" />
        <p className="text-sm text-muted-foreground italic">
          What every agent is doing — live. Updates within seconds of any
          edit to{" "}
          <code className="font-mono text-xs">
            _runtime/status/board.md
          </code>
          .
        </p>
      </header>

      {empty ? (
        <div className="border border-border bg-card p-6 text-sm text-muted-foreground">
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
            <div className="section-header">7-Day Activity</div>
            <ActivityFeed items={activity} />
          </aside>
        </div>
      )}
    </section>
  );
}
