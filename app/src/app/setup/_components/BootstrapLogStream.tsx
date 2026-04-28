"use client";

import { useEffect, useRef, useState } from "react";

/**
 * Live-tails the bootstrap log via SSE from /api/setup/bootstrap-log.
 *
 * Renders an autoscrolling pre. Falls back to the seed log (passed in
 * from the server-rendered page) if the SSE channel isn't available
 * (e.g. JS disabled).
 */
export function BootstrapLogStream({ seed }: { seed: string }) {
  const [lines, setLines] = useState<string[]>(() => splitLines(seed));
  const [done, setDone] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const preRef = useRef<HTMLPreElement | null>(null);

  useEffect(() => {
    const es = new EventSource("/api/setup/bootstrap-log");
    es.addEventListener("line", (ev) => {
      setLines((prev) => [...prev, (ev as MessageEvent).data]);
    });
    es.addEventListener("done", () => {
      setDone(true);
      es.close();
    });
    es.addEventListener("error", (ev) => {
      const data = (ev as MessageEvent).data;
      if (typeof data === "string" && data.length > 0) setError(data);
      // EventSource auto-reconnects; closing prevents loops on persistent errors.
      es.close();
    });
    return () => es.close();
  }, []);

  // Autoscroll to bottom on new lines.
  useEffect(() => {
    if (preRef.current) {
      preRef.current.scrollTop = preRef.current.scrollHeight;
    }
  }, [lines]);

  return (
    <div className="border border-border bg-secondary p-3">
      <div className="mb-2 flex items-center justify-between text-xs uppercase tracking-wider">
        <span className="text-muted-foreground">Bootstrap log</span>
        <span className={done ? "text-primary" : "text-muted-foreground"}>
          {error ? "❌ error" : done ? "✅ complete" : "● streaming…"}
        </span>
      </div>
      <pre
        ref={preRef}
        className="max-h-64 overflow-auto whitespace-pre-wrap font-mono text-[11px] leading-relaxed text-foreground/80"
      >
        {lines.length === 0 ? "(no log yet)" : lines.join("\n")}
      </pre>
      {error && (
        <p className="mt-2 text-xs text-red-600">stream error: {error}</p>
      )}
    </div>
  );
}

function splitLines(s: string): string[] {
  return s.length === 0 ? [] : s.split("\n").filter((l) => l.length > 0);
}
