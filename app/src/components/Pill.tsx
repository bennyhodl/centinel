import type { ReactNode } from "react";

export type Tone =
  | "emerald"
  | "amber"
  | "red"
  | "cyan"
  | "zinc"
  | "violet"
  | "sky";

const TONE_STYLES: Record<Tone, string> = {
  emerald: "border-emerald-700/40 text-emerald-800 bg-emerald-50",
  amber: "border-amber-700/40 text-amber-800 bg-amber-50",
  red: "border-red-700/40 text-red-800 bg-red-50",
  cyan: "border-primary/40 text-primary bg-primary/5",
  zinc: "border-foreground/20 text-muted-foreground bg-secondary",
  violet: "border-violet-700/40 text-violet-800 bg-violet-50",
  sky: "border-sky-700/40 text-sky-800 bg-sky-50",
};

export function Pill({
  tone = "zinc",
  children,
  className,
}: {
  tone?: Tone;
  children: ReactNode;
  className?: string;
}) {
  return (
    <span
      className={`inline-flex items-center border px-2 py-0.5 font-smallcaps text-[0.6rem] tracking-[0.12em] uppercase ${TONE_STYLES[tone]} ${className ?? ""}`}
    >
      {children}
    </span>
  );
}

export function statusTone(status: unknown): Tone {
  switch (status) {
    case "active":
    case "published":
    case "resolved":
      return "emerald";
    case "paused":
    case "dismissed":
      return "zinc";
    case "complete":
      return "cyan";
    case "draft":
      return "amber";
    case "raw":
      return "sky";
    case "open":
      return "amber";
    default:
      return "zinc";
  }
}
