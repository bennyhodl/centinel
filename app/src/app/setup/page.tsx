import fs from "node:fs/promises";
import { redirect } from "next/navigation";
import {
  readSetupState,
  WATCH_PRESETS,
  type SetupState,
} from "@/lib/setup-state";
import {
  submitStep1,
  submitStep2,
  submitStep3,
  submitStep4,
  startBootstrap,
  continueToActivation,
  completeSetup,
  resetSetup,
} from "./actions";
import {
  PrimaryButton,
  SecondaryButton,
  StepFrame,
  StepNav,
} from "./_components/StepShell";

export const dynamic = "force-dynamic";

interface PageProps {
  searchParams: Promise<{ done?: string }>;
}

export default async function SetupPage({ searchParams }: PageProps) {
  const sp = await searchParams;
  const state = await readSetupState();

  // If setup is already complete, show a "you're done — manage" view.
  if (state.status === "complete" && sp.done !== "1") {
    redirect("/sitemap");
  }

  return (
    <section className="mx-auto max-w-3xl space-y-6">
      <header>
        <h1 className="text-3xl font-semibold tracking-tight">
          {state.projectName ?? "Tampa-DOGE"} setup
        </h1>
        <p className="mt-1 text-sm opacity-60">
          {state.status === "complete"
            ? "Setup complete — review or re-run below."
            : "Get the Cartographer pointed at your city. Takes ~5 minutes plus a 30–90 minute bootstrap crawl."}
        </p>
      </header>

      <StepNav state={state} />

      {state.status === "complete" ? (
        <CompletedView state={state} />
      ) : (
        <CurrentStepView state={state} />
      )}
    </section>
  );
}

function CurrentStepView({ state }: { state: SetupState }) {
  switch (state.step) {
    case 1:
      return <Step1 state={state} />;
    case 2:
      return <Step2 state={state} />;
    case 3:
      return <Step3 state={state} />;
    case 4:
      return <Step4 state={state} />;
    case 5:
      return <Step5 state={state} />;
    case 6:
      return <Step6 state={state} />;
    case 7:
      return <Step7 state={state} />;
  }
}

/* ─────────────────────  STEP 1: City domain  ───────────────────── */

function Step1({ state }: { state: SetupState }) {
  return (
    <StepFrame
      title="Step 1 — City .gov domain"
      subtitle="The root domain the Cartographer will crawl. Use the canonical hostname; we'll follow sitemap.xml from here."
    >
      <form action={submitStep1} className="space-y-4">
        <label className="block">
          <span className="mb-1.5 block text-xs uppercase tracking-wider opacity-60">
            City domain
          </span>
          <input
            name="cityDomain"
            defaultValue={state.cityDomain ?? ""}
            placeholder="www.tampa.gov"
            autoFocus
            required
            className="w-full rounded-md border border-white/10 bg-black/40 px-3 py-2 font-mono text-sm placeholder:opacity-40 focus:border-tampa-cyan focus:outline-none"
          />
        </label>
        <p className="text-xs opacity-50">
          Examples: <code className="text-tampa-cyan">www.tampa.gov</code>,{" "}
          <code className="text-tampa-cyan">www.cityofstpete.org</code>
        </p>
        <div className="flex justify-end">
          <PrimaryButton>Continue →</PrimaryButton>
        </div>
      </form>
    </StepFrame>
  );
}

/* ─────────────────────  STEP 2: Branding  ───────────────────── */

function Step2({ state }: { state: SetupState }) {
  return (
    <StepFrame
      title="Step 2 — Project name"
      subtitle="Shown in the nav bar and weekly briefings. Keep the default unless you're forking for another city."
    >
      <form action={submitStep2} className="space-y-4">
        <label className="block">
          <span className="mb-1.5 block text-xs uppercase tracking-wider opacity-60">
            Project name
          </span>
          <input
            name="projectName"
            defaultValue={state.projectName ?? "Tampa-DOGE"}
            className="w-full rounded-md border border-white/10 bg-black/40 px-3 py-2 text-sm focus:border-tampa-cyan focus:outline-none"
          />
        </label>
        <p className="text-xs opacity-50">
          Logo upload deferred to v0.2 — for now, the default text mark is used.
        </p>
        <div className="flex justify-end">
          <PrimaryButton>Continue →</PrimaryButton>
        </div>
      </form>
    </StepFrame>
  );
}

/* ─────────────────────  STEP 3: Watches  ───────────────────── */

function Step3({ state }: { state: SetupState }) {
  const selected = new Set(state.watchPresets ?? []);
  return (
    <StepFrame
      title="Step 3 — Watch presets"
      subtitle="Continuous matchers the Watch Runner applies over every sitemap diff. Pick the lenses you care about — you can tune or add custom watches later."
    >
      <form action={submitStep3} className="space-y-3">
        {WATCH_PRESETS.map((p) => (
          <label
            key={p.id}
            className="flex cursor-pointer items-start gap-3 rounded-md border border-white/10 bg-white/[0.02] p-3 transition hover:bg-white/[0.04]"
          >
            <input
              type="checkbox"
              name={`preset:${p.id}`}
              defaultChecked={selected.has(p.id)}
              className="mt-0.5 h-4 w-4 accent-tampa-cyan"
            />
            <span className="flex-1">
              <span className="block font-medium">{p.label}</span>
              <span className="mt-0.5 block text-xs opacity-60">
                {p.description}
              </span>
            </span>
          </label>
        ))}
        <div className="flex justify-end pt-2">
          <PrimaryButton>Continue →</PrimaryButton>
        </div>
      </form>
    </StepFrame>
  );
}

/* ─────────────────────  STEP 4: Notifications  ───────────────────── */

function Step4({ state }: { state: SetupState }) {
  const channel = state.notification?.channel ?? "none";
  return (
    <StepFrame
      title="Step 4 — Briefing channel (optional)"
      subtitle="Where the Briefings Writer posts the weekly digest. Day-to-day investigation work happens in /chat — no push notifications until you ask."
    >
      <form action={submitStep4} className="space-y-4">
        <fieldset className="space-y-2">
          {(["none", "discord", "telegram"] as const).map((c) => (
            <label
              key={c}
              className="flex cursor-pointer items-center gap-3 rounded-md border border-white/10 bg-white/[0.02] p-3 transition hover:bg-white/[0.04]"
            >
              <input
                type="radio"
                name="channel"
                value={c}
                defaultChecked={channel === c}
                className="h-4 w-4 accent-tampa-cyan"
              />
              <span className="capitalize">
                {c === "none" ? "No channel — read on /briefings" : c}
              </span>
            </label>
          ))}
        </fieldset>
        <label className="block">
          <span className="mb-1.5 block text-xs uppercase tracking-wider opacity-60">
            Channel target (optional)
          </span>
          <input
            name="target"
            defaultValue={state.notification?.target ?? ""}
            placeholder="#tampa-doge or chat-id"
            className="w-full rounded-md border border-white/10 bg-black/40 px-3 py-2 font-mono text-sm placeholder:opacity-40 focus:border-tampa-cyan focus:outline-none"
          />
        </label>
        <div className="flex justify-end">
          <PrimaryButton>Continue →</PrimaryButton>
        </div>
      </form>
    </StepFrame>
  );
}

/* ─────────────────────  STEP 5: Bootstrap  ───────────────────── */

function Step5({ state }: { state: SetupState }) {
  const presetLabels = (state.watchPresets ?? [])
    .map((id) => WATCH_PRESETS.find((p) => p.id === id)?.label ?? id)
    .join(", ") || "none";

  return (
    <StepFrame
      title="Step 5 — Bootstrap the sitemap"
      subtitle="The Cartographer crawls every URL on the city's .gov surface, classifies content, and writes a labeled sitemap. Takes 30–90 minutes for a city like Tampa."
    >
      <div className="space-y-4">
        <dl className="grid grid-cols-1 gap-2 rounded-md border border-white/10 bg-black/30 p-4 text-sm sm:grid-cols-2">
          <Field label="Domain" value={state.cityDomain ?? "—"} mono />
          <Field label="Project" value={state.projectName ?? "Tampa-DOGE"} />
          <Field label="Watches" value={presetLabels} />
          <Field
            label="Briefings"
            value={
              state.notification?.channel === "none" || !state.notification
                ? "viewer only"
                : `${state.notification.channel}${
                    state.notification.target ? ` · ${state.notification.target}` : ""
                  }`
            }
          />
        </dl>

        <div className="rounded-md border border-amber-500/30 bg-amber-500/5 p-3 text-xs">
          <strong className="text-amber-400">Stub mode:</strong>{" "}
          <span className="opacity-80">
            The real shell-out to{" "}
            <code className="text-tampa-cyan">
              hermes session run sitemap-builder
            </code>{" "}
            isn&apos;t wired yet. This step writes a placeholder log so the
            wizard can advance and the rest of the app can be exercised end-to-end.
          </span>
        </div>

        <form action={startBootstrap} className="flex justify-end gap-2">
          <PrimaryButton>Start bootstrap (stub) →</PrimaryButton>
        </form>
      </div>
    </StepFrame>
  );
}

/* ─────────────────────  STEP 6: Review  ───────────────────── */

async function Step6({ state }: { state: SetupState }) {
  let log = "";
  if (state.bootstrap?.logPath) {
    try {
      log = await fs.readFile(state.bootstrap.logPath, "utf-8");
    } catch {
      log = "(log file not found)";
    }
  }

  return (
    <StepFrame
      title="Step 6 — Review the sitemap"
      subtitle="Skim the sitemap, mark bulk categories active, then continue. You can come back and refine forever — this is just the first pass."
    >
      <div className="space-y-4">
        <div className="rounded-md border border-white/10 bg-black/40 p-3">
          <div className="mb-2 text-xs uppercase tracking-wider opacity-60">
            Bootstrap log
          </div>
          <pre className="max-h-48 overflow-auto whitespace-pre-wrap font-mono text-[11px] leading-relaxed opacity-80">
{log || "(no log yet)"}
          </pre>
        </div>

        <div className="grid gap-2 sm:grid-cols-2">
          <a
            href="/sitemap"
            target="_blank"
            rel="noopener noreferrer"
            className="rounded-md border border-white/15 bg-white/[0.03] px-4 py-3 text-sm transition hover:bg-white/[0.08]"
          >
            Open <span className="text-tampa-cyan">/sitemap</span> →
          </a>
          <a
            href="/sitemap/needs-review"
            target="_blank"
            rel="noopener noreferrer"
            className="rounded-md border border-white/15 bg-white/[0.03] px-4 py-3 text-sm transition hover:bg-white/[0.08]"
          >
            Triage <span className="text-tampa-cyan">needs-review</span> queue →
          </a>
        </div>

        <form action={continueToActivation} className="flex justify-end">
          <PrimaryButton>Continue to activation →</PrimaryButton>
        </form>
      </div>
    </StepFrame>
  );
}

/* ─────────────────────  STEP 7: Activate  ───────────────────── */

function Step7({ state: _state }: { state: SetupState }) {
  return (
    <StepFrame
      title="Step 7 — Activate cron"
      subtitle="Flip the agent stack from paused → active. After this, the browser can close — the Cartographer, Investigator, Archivist, and Watch Runner will run on schedule forever."
    >
      <ul className="mb-5 space-y-2 text-sm">
        {[
          "Cartographer — weekly sitemap lint",
          "Investigator — hourly tick on active investigations",
          "Archivist — vaults documents as they appear",
          "Watch Runner — runs preset watches over every diff",
          "Briefings Writer — weekly digest",
        ].map((line) => (
          <li
            key={line}
            className="flex items-start gap-2 rounded-md border border-white/5 bg-white/[0.02] px-3 py-2"
          >
            <span className="mt-1 inline-block h-1.5 w-1.5 shrink-0 rounded-full bg-tampa-cyan" />
            <span>{line}</span>
          </li>
        ))}
      </ul>

      <div className="rounded-md border border-amber-500/30 bg-amber-500/5 p-3 text-xs">
        <strong className="text-amber-400">Stub mode:</strong>{" "}
        <span className="opacity-80">
          Cron registration isn&apos;t wired to Hermes yet. Marking setup
          complete unlocks the rest of the app; cron activation comes online
          once the agent skills land.
        </span>
      </div>

      <form action={completeSetup} className="mt-4 flex justify-end">
        <PrimaryButton>Activate & finish →</PrimaryButton>
      </form>
    </StepFrame>
  );
}

/* ─────────────────────  Completed view  ───────────────────── */

function CompletedView({ state }: { state: SetupState }) {
  return (
    <StepFrame
      title="Setup complete"
      subtitle={`Bootstrapped ${state.cityDomain ?? "—"} on ${
        state.completedAt?.slice(0, 10) ?? "—"
      }.`}
    >
      <p className="mb-4 text-sm opacity-80">
        The agent stack is live. Visit <a href="/sitemap" className="text-tampa-cyan hover:underline">the sitemap</a> to start working, or open <a href="/chat" className="text-tampa-cyan hover:underline">/chat</a> to talk to the Editor.
      </p>
      <form action={resetSetup} className="flex justify-end">
        <SecondaryButton>Reset setup</SecondaryButton>
      </form>
    </StepFrame>
  );
}

/* ─────────────────────  helpers  ───────────────────── */

function Field({
  label,
  value,
  mono,
}: {
  label: string;
  value: string;
  mono?: boolean;
}) {
  return (
    <div>
      <dt className="text-[10px] uppercase tracking-wider opacity-50">
        {label}
      </dt>
      <dd className={`mt-0.5 ${mono ? "font-mono" : ""}`}>{value}</dd>
    </div>
  );
}
