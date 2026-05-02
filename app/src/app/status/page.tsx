import Link from "next/link";
import { listRecentActivity, readBoard } from "@/lib/status";
import { listAllFindings } from "@/lib/findings";
import { parseBoard } from "@/lib/status-parser";
import InFlightPanel from "./_components/InFlightPanel";
import ActivityFeed from "./_components/ActivityFeed";
import { RecentFindings } from "./_components/RecentFindings";

export const dynamic = "force-dynamic";

export default async function StatusPage() {
  const [board, activity, findings] = await Promise.all([
    readBoard(),
    listRecentActivity(7),
    listAllFindings(),
  ]);

  // Pre-parse the initial board on the server so we can show meaningful
  // "0 in flight" copy without flashing the full markdown first.
  const initialParsed = parseBoard(board.body);
  const empty =
    !board.body.trim() && activity.length === 0 && findings.length === 0;

  // Show the 8 most recent findings; the full list lives at /findings.
  const recentFindings = findings.slice(0, 8);

  return (
    <section>
      <header className="mb-6">
        <div className="flex flex-wrap items-baseline justify-between gap-3">
          <div>
            <h1 className="masthead text-3xl text-foreground">Status</h1>
            <hr className="rule-double" />
            <p className="text-sm text-muted-foreground italic">
              What every agent is doing — live. The board updates within
              seconds of any agent edit; findings appear here as runs land them.
            </p>
          </div>
          <div className="flex items-baseline gap-3 text-xs text-muted-foreground">
            <span>
              <strong className="text-foreground">{initialParsed.inFlight.length}</strong>{" "}
              in flight
            </span>
            <span className="opacity-40">·</span>
            <span>
              <strong className="text-foreground">{findings.length}</strong>{" "}
              total findings
            </span>
            <span className="opacity-40">·</span>
            <span>
              <strong className="text-foreground">{activity.length}</strong>{" "}
              events / 7d
            </span>
          </div>
        </div>
      </header>

      {empty ? (
        <div className="border border-dashed border-border bg-card p-8 text-center text-sm text-muted-foreground">
          <p className="italic">No agent activity yet.</p>
          <p className="mt-2">
            Once cron activates and an agent runs (try{" "}
            <Link href="/investigations" className="text-primary hover:underline">
              creating an investigation
            </Link>{" "}
            or hitting Run Now on an existing one), this page lights up with
            live agent state, findings as they&apos;re drafted, and a
            chronological activity feed.
          </p>
        </div>
      ) : (
        <div className="space-y-8">
          {/* Top: In-flight runs (live, big) */}
          <div>
            <div className="mb-3 flex items-baseline justify-between gap-3">
              <h2 className="section-header">In flight</h2>
              <Link
                href="/investigations"
                className="text-xs text-primary hover:underline italic"
              >
                manage investigations →
              </Link>
            </div>
            <InFlightPanel
              initialMarkdown={board.body}
              initialMtime={board.mtime}
            />
          </div>

          {/* Bottom: two columns — recent findings (left, prominent) + audit feed (right) */}
          <div className="grid gap-8 lg:grid-cols-[minmax(0,3fr)_minmax(0,2fr)]">
            <div>
              <div className="mb-3 flex items-baseline justify-between gap-3">
                <h2 className="section-header">Recent findings</h2>
                <Link
                  href="/findings"
                  className="text-xs text-primary hover:underline italic"
                >
                  all findings →
                </Link>
              </div>
              <RecentFindings items={recentFindings} />
            </div>
            <aside>
              <div className="mb-3 flex items-baseline justify-between gap-3">
                <h2 className="section-header">Activity · 7d</h2>
              </div>
              <ActivityFeed items={activity} />
            </aside>
          </div>
        </div>
      )}
    </section>
  );
}
