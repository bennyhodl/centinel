"use client";

import { useEffect, useState } from "react";
import ClientMarkdown from "./ClientMarkdown";

export interface LiveBoardProps {
  initialMarkdown: string;
  initialMtime: number;
}

function relTime(ms: number): string {
  if (!ms) return "never";
  const dt = Date.now() - ms;
  if (dt < 5_000) return "just now";
  if (dt < 60_000) return `${Math.floor(dt / 1000)}s ago`;
  if (dt < 3_600_000) return `${Math.floor(dt / 60_000)}m ago`;
  if (dt < 86_400_000) return `${Math.floor(dt / 3_600_000)}h ago`;
  return `${Math.floor(dt / 86_400_000)}d ago`;
}

export default function LiveBoard({
  initialMarkdown,
  initialMtime,
}: LiveBoardProps) {
  const [markdown, setMarkdown] = useState(initialMarkdown);
  const [mtime, setMtime] = useState(initialMtime);
  const [connected, setConnected] = useState(false);
  const [tick, setTick] = useState(0);

  useEffect(() => {
    const id = setInterval(() => setTick((t) => t + 1), 5_000);
    return () => clearInterval(id);
  }, []);

  useEffect(() => {
    const es = new EventSource("/status/api/board");
    es.onopen = () => setConnected(true);
    es.onerror = () => setConnected(false);
    es.onmessage = (ev) => {
      try {
        const snap = JSON.parse(ev.data) as {
          body: string;
          mtime: number;
        };
        setMarkdown(snap.body ?? "");
        setMtime(snap.mtime ?? 0);
      } catch {
        // ignore malformed
      }
    };
    return () => es.close();
  }, []);

  // tick is read so the relative-time label re-renders
  void tick;

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
        <span className="uppercase tracking-wider text-muted-foreground">
          {connected ? "live" : "reconnecting"}
        </span>
        <span className="opacity-40">·</span>
        <span className="opacity-60">updated {relTime(mtime)}</span>
      </div>

      {markdown.trim() ? (
        <ClientMarkdown source={markdown} />
      ) : (
        <div className="border border-border bg-card p-6 text-sm text-muted-foreground">
          No board yet. Once an agent runs, it will write{" "}
          <code className="font-mono text-xs">_runtime/status/board.md</code>{" "}
          and this view will update live.
        </div>
      )}
    </div>
  );
}
