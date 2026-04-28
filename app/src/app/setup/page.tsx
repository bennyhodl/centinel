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
import { BootstrapLogStream } from "./_components/BootstrapLogStream";

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
      <header className="mb-6">
        <h1 className="masthead text-3xl text-foreground">
          {state.projectName ?? "Centinel"} Setup
        </h1>
        <hr className="rule-double" />
        <p className="text-sm text-muted-foreground italic">
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
          <span className="mb-1.5 block text-xs uppercase tracking-wider text-muted-foreground">
            City domain
          </span>
          <input
            name="cityDomain"
            defaultValue={state.cityDomain ?? ""}
            placeholder="www.tampa.gov"
            autoFocus
            required
            className="w-full border border-border bg-secondary px-3 py-2 font-mono text-sm placeholder:text-muted-foreground focus:border-primary focus:outline-none"
          />
        </label>
        <p className="text-xs text-muted-foreground">
          Examples: <code className="text-primary">www.tampa.gov</code>,{" "}
          <code className="text-primary">www.cityofstpete.org</code>
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
          <span className="mb-1.5 block text-xs uppercase tracking-wider text-muted-foreground">
            Project name
          </span>
          <input
            name="projectName"
            defaultValue={state.projectName ?? "Centinel"}
            className="w-full border border-border bg-secondary px-3 py-2 text-sm focus:border-primary focus:outline-none"
          />
        </label>
        <p className="text-xs text-muted-foreground">
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
            className="flex cursor-pointer items-start gap-3 border border-border bg-card p-3 transition hover:bg-accent"
          >
            <input
              type="checkbox"
              name={`preset:${p.id}`}
              defaultChecked={selected.has(p.id)}
              className="mt-0.5 h-4 w-4 accent-primary"
            />
            <span className="flex-1">
              <span className="block font-medium">{p.label}</span>
              <span className="mt-0.5 block text-xs text-muted-foreground">
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
              className="flex cursor-pointer items-center gap-3 border border-border bg-card p-3 transition hover:bg-accent"
            >
              <input
                type="radio"
                name="channel"
                value={c}
                defaultChecked={channel === c}
                className="h-4 w-4 accent-primary"
              />
              <span className="capitalize">
                {c === "none" ? "No channel — read on /briefings" : c}
              </span>
            </label>
          ))}
        </fieldset>
        <label className="block">
          <span className="mb-1.5 block text-xs uppercase tracking-wider text-muted-foreground">
            Channel target (optional)
          </span>
          <input
            name="target"
            defaultValue={state.notification?.target ?? ""}
            placeholder="#centinel or chat-id"
            className="w-full border border-border bg-secondary px-3 py-2 font-mono text-sm placeholder:text-muted-foreground focus:border-primary focus:outline-none"
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
        <dl className="grid grid-cols-1 gap-2 border border-border bg-secondary p-4 text-sm sm:grid-cols-2">
          <Field label="Domain" value={state.cityDomain ?? "—"} mono />
          <Field label="Project" value={state.projectName ?? "Centinel"} />
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

        <div className="border border-border bg-card p-3 text-xs">
          <strong className="text-foreground">Live shell-out:</strong>{" "}
          <span className="text-muted-foreground">
            Pressing the button spawns{" "}
            <code className="text-primary">
              ../bin/centinel bootstrap-sitemap {state.cityDomain ?? "<domain>"}
            </code>{" "}
            detached. Step 6 streams the log via SSE so you can watch progress in real time. Closing the browser does not stop the bootstrap.
          </span>
        </div>

        <form action={startBootstrap} className="flex justify-end gap-2">
          <PrimaryButton>Start bootstrap →</PrimaryButton>
        </form>
      </div>
    </StepFrame>
  );
}

/* ─────────────────────  STEP 6: Review  ───────────────────── */

async function Step6({ state }: { state: SetupState }) {
  let seed = "";
  if (state.bootstrap?.logPath) {
    try {
      seed = await fs.readFile(state.bootstrap.logPath, "utf-8");
    } catch {
      seed = "(log file not found)";
    }
  }

  return (
    <StepFrame
      title="Step 6 — Review the sitemap"
      subtitle="Skim the sitemap, mark bulk categories active, then continue. You can come back and refine forever — this is just the first pass."
    >
      <div className="space-y-4">
        <BootstrapLogStream seed={seed} />

        <div className="grid gap-2 sm:grid-cols-2">
          <a
            href="/sitemap"
            target="_blank"
            rel="noopener noreferrer"
            className="border border-border bg-card px-4 py-3 text-sm transition hover:bg-accent"
          >
            Open <span className="text-primary">/sitemap</span> →
          </a>
          <a
            href="/sitemap/needs-review"
            target="_blank"
            rel="noopener noreferrer"
            className="border border-border bg-card px-4 py-3 text-sm transition hover:bg-accent"
          >
            Triage <span className="text-primary">needs-review</span> queue →
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

function Step7({ state }: { state: SetupState }) {
  const lastError = state.activation?.error;
  return (
    <StepFrame
      title="Step 7 — Activate cron"
      subtitle="Flip the agent stack from paused → active. After this, the browser can close — the Cartographer, Investigator, Archivist, and Watch Runner will run on schedule forever."
    >
      <ul className="mb-5 space-y-2 text-sm">
        {[
          "Cartographer — weekly sitemap lint",
          "Investigator — every-4h tick on inbox + active investigations",
          "Archivist — vaults documents as they appear",
          "Watch Runner — runs preset watches over every diff",
          "Data Reporter — refreshes entity DB every 6h",
          "Briefings Writer — weekly digest",
        ].map((line) => (
          <li
            key={line}
            className="flex items-start gap-2 border border-border/50 bg-card px-3 py-2"
          >
            <span className="mt-1 inline-block h-1.5 w-1.5 shrink-0 bg-primary" />
            <span>{line}</span>
          </li>
        ))}
      </ul>

      <div className="border border-border bg-card p-3 text-xs">
        <strong className="text-foreground">Live activation:</strong>{" "}
        <span className="text-muted-foreground">
          Submitting runs{" "}
          <code className="text-primary">../bin/centinel cron resume-all</code>{" "}
          synchronously, which flips every Centinel-owned cron job from paused →
          active across all profiles. Returns in ~1s.
        </span>
      </div>

      {lastError && (
        <div className="mt-3 border border-red-300 bg-red-50 p-3 text-xs">
          <strong className="text-red-600">Last activation failed:</strong>{" "}
          <span className="opacity-80">{lastError}</span>
          {state.activation?.output && (
            <pre className="mt-2 max-h-32 overflow-auto whitespace-pre-wrap font-mono text-[10px] leading-relaxed">
              {state.activation.output}
            </pre>
          )}
        </div>
      )}

      <form action={completeSetup} className="mt-4 flex justify-end">
        <PrimaryButton>
          {lastError ? "Retry activation →" : "Activate & finish →"}
        </PrimaryButton>
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
      <p className="mb-4 text-sm text-foreground/80">
        The agent stack is live. Visit <a href="/sitemap" className="text-primary hover:underline">the sitemap</a> to start working, or open <a href="/chat" className="text-primary hover:underline">/chat</a> to talk to the Editor.
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
      <dt className="text-[10px] uppercase tracking-wider text-muted-foreground">
        {label}
      </dt>
      <dd className={`mt-0.5 ${mono ? "font-mono" : ""}`}>{value}</dd>
    </div>
  );
}
