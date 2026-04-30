"use client";

import { useEffect, useState } from "react";

/**
 * Render an ISO timestamp in the user's browser locale + timezone.
 *
 * SSR-safe: shows the raw ISO string until hydration completes, then swaps to
 * the formatted local string. No layout shift past the first paint.
 *
 * Defaults: medium date + short time (e.g. "Apr 30, 2026, 8:00 PM EDT").
 */
export function LocalTime({
  iso,
  mode = "datetime",
  showRelative = false,
  className,
}: {
  iso: string;
  mode?: "datetime" | "date" | "time";
  showRelative?: boolean;
  className?: string;
}) {
  const [formatted, setFormatted] = useState<string | null>(null);
  const [relative, setRelative] = useState<string | null>(null);

  useEffect(() => {
    const d = new Date(iso);
    if (Number.isNaN(d.getTime())) {
      setFormatted(iso);
      return;
    }

    const opts: Intl.DateTimeFormatOptions =
      mode === "date"
        ? { dateStyle: "medium" }
        : mode === "time"
          ? { timeStyle: "short" }
          : {
              // Explicit fields — `dateStyle`/`timeStyle` can't be combined
              // with `timeZoneName` (throws "Invalid option" in Chrome).
              year: "numeric",
              month: "short",
              day: "numeric",
              hour: "numeric",
              minute: "2-digit",
              timeZoneName: "short",
            };

    setFormatted(new Intl.DateTimeFormat(undefined, opts).format(d));

    if (showRelative) {
      const update = () => setRelative(formatRelative(d));
      update();
      const id = setInterval(update, 60_000);
      return () => clearInterval(id);
    }
  }, [iso, mode, showRelative]);

  // SSR / pre-hydration: show ISO so the markup is deterministic.
  if (formatted === null) {
    return (
      <time className={className} dateTime={iso} suppressHydrationWarning>
        {iso}
      </time>
    );
  }

  return (
    <time className={className} dateTime={iso} suppressHydrationWarning>
      {formatted}
      {showRelative && relative && (
        <span className="ml-1.5 text-muted-foreground">({relative})</span>
      )}
    </time>
  );
}

function formatRelative(d: Date): string {
  const diffSec = Math.round((d.getTime() - Date.now()) / 1000);
  const abs = Math.abs(diffSec);
  const rtf = new Intl.RelativeTimeFormat(undefined, { numeric: "auto" });
  if (abs < 60) return rtf.format(diffSec, "second");
  if (abs < 3600) return rtf.format(Math.round(diffSec / 60), "minute");
  if (abs < 86400) return rtf.format(Math.round(diffSec / 3600), "hour");
  return rtf.format(Math.round(diffSec / 86400), "day");
}
