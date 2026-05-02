"use client";

import { useEffect, useState } from "react";
import Link from "next/link";
import {
  parseBoard,
  type BoardSections,
  type InFlightRun,
} from "@/lib/status-parser";

export interface InFlightPanelProps {
  initialMarkdown: string;
  initialMtime: number;
}

function fmtRunningFor(startedIso: string | null): string | null {
  if (!startedIso) return null;
  const t = Date.parse(startedIso);
  if (Number.isNaN(t)) return null;
  const dt = Date.now() - t;
  if (dt < 0) return "just started";
  if (dt < 60_000) return `${Math.floor(dt / 1000)}s`;
  if (dt < 3_600_000) return `${Math.floor(dt / 60_000)}m`;
  return `${Math.floor(dt / 3_600_000)}h ${Math.floor((dt % 3_600_000) / 60_000)}m`;
}

function fmtRel(mtime: number): string {
  if (!mtime) return "never";
  const dt = Date.now() - mtime;
  if (dt < 5_000) return "just now";
  if (dt < 60_000) return `${Math.floor(dt / 1000)}s ago`;
  if (dt < 3_600_000) return `${Math.floor(dt / 60_000)}m ago`;
  if (dt < 86_400_000) return `${Math.floor(dt / 3_600_000)}h ago`;
  return `${Math.floor(dt / 86_400_000)}d ago`;
}

const AGENT_TONE: Record<string, string> = {
  Investigator: "border-cyan-400/50 bg-cyan-50 text-cyan-900",
  Cartographer: "border-violet-400/50 bg-violet-50 text-violet-900",
  "Watch-runner": "border-amber-400/50 bg-amber-50 text-amber-900",
  Watcher: "border-amber-400/50 bg-amber-50 text-amber-900",
  "Data-reporter": "border-emerald-400/50 bg-emerald-50 text-emerald-900",
  Reporter: "border-emerald-400/50 bg-emerald-50 text-emerald-900",
  Archivist: "border-zinc-400/50 bg-zinc-50 text-zinc-900",
  Editor: "border-primary/50 bg-primary/10 text-primary",
};

function RunCard({ run }: { run: InFlightRun }) {
  const tone =
    (run.agent && AGENT_TONE[run.agent]) ??
    "border-border bg-secondary/40 text-foreground/80";
  const runningFor = fmtRunningFor(run.startedIso);

  return (
    <div className="border border-border bg-card overflow-hidden">
      <div className="flex flex-wrap items-baseline gap-2 px-4 pt-3">
        <span
          className={`border px-2 py-0.5 font-smallcaps text-[0.6rem] tracking-[0.12em] uppercase ${tone}`}
        >
          {run.agent ?? "agent"}
        </span>
        {run.target && (
          <Link
            href={`/investigations/${run.target}`}
            className="font-mono text-sm text-primary hover:underline"
          >
            {run.target}
          </Link>
        )}
        <span className="ml-auto flex items-center gap-2 text-[0.65rem] text-muted-foreground">
          <span className="relative inline-flex h-2 w-2">
            <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-emerald-400 opacity-60" />
            <span className="relative inline-flex h-2 w-2 rounded-full bg-emerald-500" />
          </span>
          {runningFor ? `running for ${runningFor}` : "in flight"}
        </span>
      </div>
      <div className="px-4 py-2">
        <p className="text-sm text-foreground/80 leading-relaxed">
          {run.detail ?? run.raw}
        </p>
      </div>
      {run.startedIso && (
        <div className="border-t border-border bg-secondary/30 px-4 py-1.5 text-[0.6rem] text-muted-foreground">
          started {new Date(run.startedIso).toLocaleString()}
        </div>
      )}
    </div>
  );
}

export default function InFlightPanel({
  initialMarkdown,
  initialMtime,
}: InFlightPanelProps) {
  const [markdown, setMarkdown] = useState(initialMarkdown);
  const [mtime, setMtime] = useState(initialMtime);
  const [connected, setConnected] = useState(false);
  const [tick, setTick] = useState(0);

  // Re-render relative timestamps every 5s so "running for 32s" actually moves.
  useEffect(() => {
    const id = setInterval(() => setTick((t) => t + 1), 5_000);
    return () => clearInterval(id);
  }, []);

  // SSE stream from the existing /status/api/board endpoint.
  useEffect(() => {
    const es = new EventSource("/status/api/board");
    es.onopen = () => setConnected(true);
    es.onerror = () => setConnected(false);
    es.onmessage = (ev) => {
      try {
        const snap = JSON.parse(ev.data) as { body: string; mtime: number };
        setMarkdown(snap.body ?? "");
        setMtime(snap.mtime ?? 0);
      } catch {
        /* ignore */
      }
    };
    return () => es.close();
  }, []);

  void tick;

  const board: BoardSections = parseBoard(markdown);

  return (
    <div className="space-y-3">
      <div className="flex items-center gap-2 text-xs">
        <span
          className={`relative inline-flex h-2.5 w-2.5 ${
            connected ? "" : "opacity-40"
          }`}
          aria-hidden
        >
          {connected && (
            <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-emerald-400 opacity-60" />
          )}
          <span
            className={`relative inline-flex h-2.5 w-2.5 rounded-full ${
              connected ? "bg-emerald-400" : "bg-zinc-500"
            }`}
          />
        </span>
        <span className="font-smallcaps tracking-[0.15em] text-muted-foreground uppercase">
          {connected ? "live" : "reconnecting"}
        </span>
        <span className="opacity-40">·</span>
        <span className="text-muted-foreground">
          board updated {fmtRel(mtime)}
        </span>
      </div>

      {board.inFlight.length === 0 ? (
        <div className="border border-dashed border-border bg-card/60 p-6 text-center text-sm text-muted-foreground">
          <p className="italic">No agents currently in flight.</p>
          <p className="mt-1 text-xs">
            Idle is normal between scheduled ticks. Agents update this board
            when they begin a run.
          </p>
        </div>
      ) : (
        <div className="grid gap-3">
          {board.inFlight.map((r, i) => (
            <RunCard key={`${r.raw}-${i}`} run={r} />
          ))}
        </div>
      )}

      {board.recent.length > 0 && (
        <details className="border border-border bg-card">
          <summary className="cursor-pointer px-4 py-2 text-xs text-muted-foreground hover:bg-accent">
            <span className="font-smallcaps tracking-[0.12em] uppercase">
              Last 24h
            </span>
            <span className="ml-2 opacity-60">
              {board.recent.length}{" "}
              {board.recent.length === 1 ? "entry" : "entries"}
            </span>
          </summary>
          <ul className="divide-y divide-border">
            {board.recent.map((r, i) => (
              <li
                key={i}
                className="px-4 py-2 text-sm text-foreground/80 leading-relaxed"
              >
                {r.agent && (
                  <span
                    className={`mr-2 border px-1.5 py-0.5 font-smallcaps text-[0.55rem] tracking-[0.12em] uppercase ${
                      AGENT_TONE[r.agent] ?? "border-border bg-secondary"
                    }`}
                  >
                    {r.agent}
                  </span>
                )}
                {r.raw}
              </li>
            ))}
          </ul>
        </details>
      )}
    </div>
  );
}
