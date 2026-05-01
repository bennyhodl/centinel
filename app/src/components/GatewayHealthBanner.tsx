"use client";

import { useEffect, useState } from "react";

interface GatewayStatus {
  running: boolean;
  detail: string;
}

/**
 * Global banner shown ONLY when the Hermes scheduler is down. The scheduler
 * is what actually fires cron jobs (scheduled + Run-Now triggers); without
 * it, all "trigger" actions silently do nothing. This banner exists so the
 * operator sees that state on every page before they waste clicks.
 *
 * Polls every 30s. Keeps quiet when everything is green to stay out of the
 * way during normal operation.
 */
export function GatewayHealthBanner() {
  const [status, setStatus] = useState<GatewayStatus | null>(null);

  useEffect(() => {
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | null = null;

    async function check() {
      try {
        const res = await fetch("/api/gateway-status", { cache: "no-store" });
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        const data = (await res.json()) as GatewayStatus;
        if (!cancelled) setStatus(data);
      } catch {
        // Don't claim the gateway is down on a transient network blip —
        // leave the banner hidden until we have a definitive negative.
      } finally {
        if (!cancelled) timer = setTimeout(check, 30_000);
      }
    }

    check();
    return () => {
      cancelled = true;
      if (timer) clearTimeout(timer);
    };
  }, []);

  if (!status || status.running) return null;

  return (
    <div className="border-b border-amber-500/40 bg-amber-50 px-4 py-2 text-sm text-amber-900">
      <div className="mx-auto flex max-w-6xl flex-wrap items-baseline gap-x-3 gap-y-1">
        <span className="font-smallcaps tracking-[0.12em] text-[0.65rem] uppercase">
          ⚠ Scheduler down
        </span>
        <span className="flex-1 min-w-[16rem]">
          The Hermes scheduler is not running, so cron jobs and Run-Now
          triggers won&apos;t actually fire. Start it with{" "}
          <code className="font-mono text-xs">hermes gateway start</code> or{" "}
          <code className="font-mono text-xs">
            systemctl --user start hermes-gateway
          </code>
          .
        </span>
        <details className="text-xs">
          <summary className="cursor-pointer hover:underline">detail</summary>
          <pre className="mt-1 max-w-full overflow-auto whitespace-pre-wrap break-words border border-amber-300 bg-white p-2 font-mono text-[0.7rem] text-amber-900">
            {status.detail}
          </pre>
        </details>
      </div>
    </div>
  );
}
