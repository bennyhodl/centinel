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
  emerald: "bg-emerald-500/10 text-emerald-400 ring-1 ring-emerald-500/20",
  amber: "bg-amber-500/10 text-amber-400 ring-1 ring-amber-500/20",
  red: "bg-red-500/10 text-red-400 ring-1 ring-red-500/20",
  cyan: "bg-tampa-cyan/10 text-tampa-cyan ring-1 ring-tampa-cyan/20",
  zinc: "bg-zinc-500/10 text-zinc-400 ring-1 ring-zinc-500/20",
  violet: "bg-violet-500/10 text-violet-400 ring-1 ring-violet-500/20",
  sky: "bg-sky-500/10 text-sky-400 ring-1 ring-sky-500/20",
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
      className={`inline-flex items-center rounded-full px-2 py-0.5 text-[10px] font-medium uppercase tracking-wider ${TONE_STYLES[tone]} ${className ?? ""}`}
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
