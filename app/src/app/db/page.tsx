import { dbPath } from "@/lib/config";

export const dynamic = "force-dynamic";
export const revalidate = 10;

const DEFAULT_URL = "http://localhost:8001";

function datasetteUrl(): string {
  return process.env.DATASETTE_URL || DEFAULT_URL;
}

async function probeDatasette(url: string): Promise<boolean> {
  const ctrl = new AbortController();
  const timer = setTimeout(() => ctrl.abort(), 2000);
  try {
    const res = await fetch(`${url.replace(/\/$/, "")}/-/versions.json`, {
      signal: ctrl.signal,
      cache: "no-store",
    });
    return res.ok;
  } catch {
    return false;
  } finally {
    clearTimeout(timer);
  }
}

const COMPOSE_SNIPPET = `datasette:
  image: datasetteproject/datasette:latest
  volumes:
    - <wiki>/_data:/data:ro
    - <wiki>/_data/public-views.sql:/views.sql:ro
  command: >
    datasette /data/tampa.db
    --immutable /data/tampa.db
    --metadata /views.sql
    --host 0.0.0.0 --port 8001
  ports:
    - '8001:8001'
`;

export default async function DbPage() {
  const url = datasetteUrl();
  const up = await probeDatasette(url);

  if (up) {
    return (
      <section className="-mx-4 flex h-[calc(100vh-8rem)] flex-col">
        <div className="flex flex-wrap items-center justify-between gap-3 border-b border-white/10 px-4 py-2 text-xs">
          <div className="flex items-center gap-2">
            <span className="inline-flex h-2 w-2 rounded-full bg-emerald-400" />
            <span className="opacity-60">datasette</span>
            <code className="font-mono text-white/80">{url}</code>
          </div>
          <div className="flex items-center gap-3">
            <span className="opacity-60">
              read-only · sanitized public view
            </span>
            <a
              href={url}
              target="_blank"
              rel="noreferrer noopener"
              className="text-tampa-cyan hover:underline"
            >
              open in new tab ↗
            </a>
          </div>
        </div>
        <iframe
          src={url}
          title="Datasette"
          className="flex-1 w-full bg-white"
        />
      </section>
    );
  }

  return (
    <section className="space-y-6">
      <header>
        <h1 className="text-2xl font-semibold">Database</h1>
        <p className="mt-1 text-sm opacity-60">
          Datasette is not running. Bring it up and this page will embed it.
        </p>
      </header>

      <div className="rounded-lg border border-amber-400/30 bg-amber-500/5 p-4 text-sm">
        <div className="flex items-center gap-2">
          <span className="inline-flex h-2 w-2 rounded-full bg-amber-400" />
          <span className="font-semibold uppercase tracking-wider text-amber-300">
            offline
          </span>
        </div>
        <p className="mt-2 opacity-80">
          Probed{" "}
          <code className="font-mono text-xs">
            {url}/-/versions.json
          </code>{" "}
          (2s timeout) — no response.
        </p>
      </div>

      <div className="grid gap-3 sm:grid-cols-2">
        <Field
          label="DATASETTE_URL"
          value={url}
          hint={`env var · default ${DEFAULT_URL}`}
        />
        <Field label="DB path" value={dbPath()} hint="config.dbPath()" />
      </div>

      <div>
        <h2 className="mb-2 text-sm font-semibold uppercase tracking-wider opacity-60">
          docker compose snippet
        </h2>
        <pre className="overflow-x-auto rounded-lg border border-white/10 bg-black/40 p-4 font-mono text-xs leading-relaxed">
          {COMPOSE_SNIPPET}
        </pre>
        <p className="mt-2 text-xs opacity-60">
          Replace <code className="font-mono">&lt;wiki&gt;</code> with your
          wiki path. The container mounts{" "}
          <code className="font-mono">tampa.db</code> read-only and applies{" "}
          <code className="font-mono">public-views.sql</code> as metadata so
          only sanitized views are exposed (confidence ≥ 0.7, no in-progress
          confidential investigations). See{" "}
          <code className="font-mono">WEB_APP_DESIGN.md</code> §{" "}
          <em>/db — the database explorer</em> for the full rationale.
        </p>
      </div>
    </section>
  );
}

function Field({
  label,
  value,
  hint,
}: {
  label: string;
  value: string;
  hint?: string;
}) {
  return (
    <div className="rounded-lg border border-white/10 bg-white/[0.02] p-3">
      <div className="text-xs uppercase tracking-wider opacity-60">
        {label}
      </div>
      <div className="mt-1 break-all font-mono text-sm">{value}</div>
      {hint && <div className="mt-1 text-xs opacity-50">{hint}</div>}
    </div>
  );
}
