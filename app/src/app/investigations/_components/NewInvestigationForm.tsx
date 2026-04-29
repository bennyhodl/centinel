"use client";

import { useState, useTransition } from "react";
import { createInvestigationAction } from "../actions";

export function NewInvestigationForm() {
  const [open, setOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [isPending, startTransition] = useTransition();

  function handleSubmit(formData: FormData) {
    setError(null);
    startTransition(async () => {
      try {
        await createInvestigationAction(formData);
      } catch (e) {
        // next/navigation redirect throws a special error — let it propagate.
        const message = e instanceof Error ? e.message : String(e);
        if (message.includes("NEXT_REDIRECT")) throw e;
        setError(message);
      }
    });
  }

  if (!open) {
    return (
      <div className="flex justify-end mb-4">
        <button
          type="button"
          onClick={() => setOpen(true)}
          className="border border-primary/40 bg-primary/5 px-3 py-1.5 text-sm text-primary transition hover:bg-primary/10 font-smallcaps tracking-wider"
        >
          + New Investigation
        </button>
      </div>
    );
  }

  return (
    <div className="border border-border bg-card p-6 mb-6">
      <div className="flex items-baseline justify-between mb-4">
        <h2 className="font-display text-lg font-semibold">New Investigation</h2>
        <button
          type="button"
          onClick={() => setOpen(false)}
          className="text-xs text-muted-foreground hover:text-foreground"
          disabled={isPending}
        >
          cancel
        </button>
      </div>

      <form action={handleSubmit} className="space-y-4">
        <Field label="Title" hint="A short headline. Becomes the slug.">
          <input
            type="text"
            name="title"
            required
            minLength={3}
            maxLength={200}
            placeholder="ACME Construction contract relationships"
            className="w-full border border-border bg-background px-3 py-2 text-sm focus:border-primary focus:outline-none"
            disabled={isPending}
          />
        </Field>

        <Field
          label="Goal"
          hint="One paragraph. What question does this answer? What does 'done' look like? Vague goals → vague crawls."
        >
          <textarea
            name="goal"
            required
            minLength={10}
            maxLength={4000}
            rows={4}
            placeholder="Determine whether ACME Construction, BlueRock Builders, and CityWide LLC share principals or addresses, and map their FY22-FY25 contract awards from the city."
            className="w-full border border-border bg-background px-3 py-2 text-sm font-mono focus:border-primary focus:outline-none"
            disabled={isPending}
          />
        </Field>

        <Field
          label="Seed URLs"
          hint="One URL per line. Public .gov pages — contractor registries, contract awards, council minutes. The agent fans out from these."
        >
          <textarea
            name="seeds"
            rows={4}
            placeholder={
              "https://www.tampa.gov/contracting-department/awards/\nhttps://www.tampa.gov/finance/transactions"
            }
            className="w-full border border-border bg-background px-3 py-2 text-sm font-mono focus:border-primary focus:outline-none"
            disabled={isPending}
          />
        </Field>

        <div className="grid grid-cols-2 gap-4">
          <Field label="Schedule" hint="How often the Investigator re-runs.">
            <select
              name="schedule"
              defaultValue="weekly"
              className="w-full border border-border bg-background px-3 py-2 text-sm focus:border-primary focus:outline-none"
              disabled={isPending}
            >
              <option value="daily">daily</option>
              <option value="weekly">weekly</option>
              <option value="monthly">monthly</option>
              <option value="manual">manual (operator-triggered)</option>
            </select>
          </Field>

          <Field label="Depth" hint="Max hops from seeds (1–5).">
            <input
              type="number"
              name="depth"
              min={1}
              max={5}
              defaultValue={2}
              required
              className="w-full border border-border bg-background px-3 py-2 text-sm font-mono focus:border-primary focus:outline-none"
              disabled={isPending}
            />
          </Field>
        </div>

        {error && (
          <div className="border border-destructive/40 bg-destructive/5 p-3 text-sm text-destructive">
            <pre className="whitespace-pre-wrap break-words font-mono text-xs">
              {error}
            </pre>
          </div>
        )}

        <div className="flex items-center justify-end gap-3 pt-2">
          <button
            type="button"
            onClick={() => setOpen(false)}
            disabled={isPending}
            className="border border-border bg-background px-3 py-1.5 text-sm font-smallcaps tracking-wider disabled:opacity-50"
          >
            Cancel
          </button>
          <button
            type="submit"
            disabled={isPending}
            className="border border-primary/40 bg-primary/5 px-3 py-1.5 text-sm text-primary transition hover:bg-primary/10 font-smallcaps tracking-wider disabled:opacity-50"
          >
            {isPending ? "Creating…" : "Create & Register"}
          </button>
        </div>
      </form>
    </div>
  );
}

function Field({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <div>
      <label className="block">
        <div className="font-smallcaps text-[0.6rem] tracking-[0.12em] text-muted-foreground mb-1">
          {label}
        </div>
        {children}
      </label>
      {hint && (
        <p className="mt-1 text-[0.7rem] text-muted-foreground italic leading-snug">
          {hint}
        </p>
      )}
    </div>
  );
}
