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
                ? "bg-accent ring-1 ring-primary/40"
                : reached
                  ? "bg-secondary"
                  : "bg-card text-muted-foreground"
            }`}
          >
            <span
              className={`flex h-6 w-6 items-center justify-center font-mono text-xs ${
                reached
                  ? "bg-primary text-primary-foreground"
                  : "bg-secondary text-muted-foreground"
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
    <div className="border border-border bg-card p-6">
      <header className="mb-5 border-b border-border pb-4">
        <h2 className="text-xl font-semibold">{title}</h2>
        {subtitle && (
          <p className="mt-1 text-sm text-muted-foreground">{subtitle}</p>
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
      className="bg-primary px-4 py-2 text-sm font-medium text-primary-foreground transition hover:opacity-90"
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
      className="border border-border bg-card px-4 py-2 text-sm font-medium transition hover:bg-accent"
    >
      {children}
    </button>
  );
}
