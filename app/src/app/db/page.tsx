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
        <div className="flex flex-wrap items-center justify-between gap-3 border-b border-border px-4 py-2 text-xs">
          <div className="flex items-center gap-2">
            <span className="inline-flex h-2 w-2 rounded-full bg-emerald-400" />
            <span className="text-muted-foreground">datasette</span>
            <code className="font-mono text-foreground/80">{url}</code>
          </div>
          <div className="flex items-center gap-3">
            <span className="opacity-60">
              read-only · sanitized public view
            </span>
            <a
              href={url}
              target="_blank"
              rel="noreferrer noopener"
              className="text-primary hover:underline"
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
        <h1 className="masthead text-3xl text-foreground">Database</h1>
        <p className="mt-1 text-sm text-muted-foreground">
          Datasette is not running. Bring it up and this page will embed it.
        </p>
      </header>

      <div className="border border-amber-300 bg-amber-50 p-4 text-sm">
        <div className="flex items-center gap-2">
          <span className="inline-flex h-2 w-2 rounded-full bg-amber-500" />
          <span className="font-semibold uppercase tracking-wider text-amber-700">
            offline
          </span>
        </div>
        <p className="mt-2 text-foreground/80">
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
        <h2 className="mb-2 section-header">
          docker compose snippet
        </h2>
        <pre className="overflow-x-auto border border-border bg-secondary p-4 font-mono text-xs leading-relaxed">
          {COMPOSE_SNIPPET}
        </pre>
        <p className="mt-2 text-xs text-muted-foreground">
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
    <div className="border border-border bg-card p-3">
      <div className="text-xs uppercase tracking-wider text-muted-foreground">
        {label}
      </div>
      <div className="mt-1 break-all font-mono text-sm">{value}</div>
      {hint && <div className="mt-1 text-xs text-muted-foreground">{hint}</div>}
    </div>
  );
}
