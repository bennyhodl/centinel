import { jumpToStep } from "../actions";
import type { SetupState } from "@/lib/setup-state";

const STEPS = [
  { n: 1, label: "City" },
  { n: 2, label: "Branding" },
  { n: 3, label: "Watches" },
  { n: 4, label: "Notify" },
  { n: 5, label: "Bootstrap" },
  { n: 6, label: "Review" },
  { n: 7, label: "Activate" },
] as const;

export function StepNav({ state }: { state: SetupState }) {
  const current = state.step;
  return (
    <ol className="grid grid-cols-7 gap-1">
      {STEPS.map((s) => {
        const reached = s.n <= current;
        const active = s.n === current;
        const canJump = s.n < current;
        const inner = (
          <div
            className={`flex flex-col items-center gap-1 rounded-md px-2 py-2 text-center transition ${
              active
                ? "bg-tampa-cyan/15 ring-1 ring-tampa-cyan/40"
                : reached
                  ? "bg-white/5"
                  : "bg-white/[0.02] opacity-40"
            }`}
          >
            <span
              className={`flex h-6 w-6 items-center justify-center rounded-full font-mono text-xs ${
                reached
                  ? "bg-tampa-cyan text-tampa-ink"
                  : "bg-white/10 text-white/50"
              }`}
            >
              {s.n}
            </span>
            <span className="text-[11px] uppercase tracking-wider">
              {s.label}
            </span>
          </div>
        );
        return (
          <li key={s.n}>
            {canJump ? (
              <form action={jumpToStep}>
                <input type="hidden" name="step" value={s.n} />
                <button type="submit" className="block w-full hover:opacity-90">
                  {inner}
                </button>
              </form>
            ) : (
              inner
            )}
          </li>
        );
      })}
    </ol>
  );
}

export function StepFrame({
  title,
  subtitle,
  children,
}: {
  title: string;
  subtitle?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="rounded-xl border border-white/10 bg-white/[0.02] p-6">
      <header className="mb-5 border-b border-white/10 pb-4">
        <h2 className="text-xl font-semibold">{title}</h2>
        {subtitle && (
          <p className="mt-1 text-sm opacity-60">{subtitle}</p>
        )}
      </header>
      {children}
    </div>
  );
}

export function PrimaryButton({ children }: { children: React.ReactNode }) {
  return (
    <button
      type="submit"
      className="rounded-md bg-tampa-cyan px-4 py-2 text-sm font-medium text-tampa-ink transition hover:opacity-90"
    >
      {children}
    </button>
  );
}

export function SecondaryButton({
  children,
  ...props
}: React.ButtonHTMLAttributes<HTMLButtonElement>) {
  return (
    <button
      {...props}
      className="rounded-md border border-white/15 bg-white/[0.03] px-4 py-2 text-sm font-medium transition hover:bg-white/[0.08]"
    >
      {children}
    </button>
  );
}
