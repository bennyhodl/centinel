import type { ReactNode } from 'react'
import { useQuery } from '@tanstack/react-query'
import { RefreshCw } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { queries } from './queries'
import { bytes, clock, number, when } from './format'
import { ErrorBox, PageHeader, SectionRule } from './ui'

export function SystemHealthPage() {
  const query = useQuery(queries.systemHealth())
  const health = query.data
  const doctor = health?.doctor

  return <div className="mx-auto w-full max-w-5xl">
    <PageHeader title="System health" detail="Host readiness, corpus storage and collection activity.">
      <Button variant="outline" size="sm" disabled={query.isFetching} onClick={() => void query.refetch()}>
        <RefreshCw className={query.isFetching ? 'animate-spin' : ''} />{query.isFetching ? 'Checking…' : 'Refresh'}
      </Button>
    </PageHeader>
    {query.error && <ErrorBox error={query.error} />}
    {!health || !doctor ? !query.error && <p className="text-sm text-muted-foreground" role="status">Reading diagnostics…</p> : <div className="flex flex-col gap-8">
      <div className="flex flex-wrap items-center justify-between gap-3 text-sm" aria-live="polite">
        <Readiness ready={doctor.ready} good="Host ready" bad="Host needs attention" />
        <span className="text-xs text-muted-foreground">Checked {clock(health.checked_at)} · Refreshes every minute</span>
      </div>

      <section>
        <SectionRule>Configuration</SectionRule>
        <dl className="divide-y divide-border text-sm">
          <Detail label="Store"><code className="break-all text-xs">{doctor.store_root}</code></Detail>
          <Detail label="Configuration"><code className="break-all text-xs">{doctor.config ?? 'Defaults in use'}</code></Detail>
        </dl>
        {doctor.config_searched?.length ? <p className="mt-2 break-all text-xs text-muted-foreground">No config found. Searched {doctor.config_searched.join(', ')}.</p> : null}
        {doctor.config_error && <Problem>{doctor.config_error}</Problem>}
      </section>

      <section>
        <SectionRule aside={<Readiness ready={doctor.binaries_ready} good="Required binaries found" bad="Required binaries missing" />}>Binaries</SectionRule>
        <div className="divide-y divide-border">
          {doctor.binaries.map(binary => <div key={binary.name} className="grid gap-2 py-4 sm:grid-cols-[10rem_minmax(0,1fr)]">
            <div><h3 className="font-mono text-sm">{binary.name}</h3><span className="text-xs capitalize text-muted-foreground">{binary.need}</span></div>
            <div className="min-w-0 text-sm">
              <p>{binary.path ? 'Installed' : binary.need === 'planned' ? 'Not installed; not used yet' : `Missing. Install ${binary.name} and make it available on the server's PATH.`}</p>
              <p className="mt-1 text-xs text-muted-foreground">{binary.purpose}</p>
              {binary.path && <p className="mt-1 break-all font-mono text-xs text-muted-foreground">{binary.path}</p>}
              {binary.version && <p className="mt-1 break-all font-mono text-xs text-muted-foreground">{binary.version}</p>}
              {binary.stale && <Problem>{binary.stale}</Problem>}
            </div>
          </div>)}
        </div>
      </section>

      <section>
        <SectionRule aside={<Readiness ready={doctor.services_ready} />}>Model servers</SectionRule>
        <p className="mt-3 text-xs text-muted-foreground">Doctor checks configured keys and server reachability without making model calls.</p>
        <div className="divide-y divide-border">
          {doctor.services.map(service => <div key={service.capability} className="grid gap-2 py-4 sm:grid-cols-[10rem_minmax(0,1fr)]">
            <div><h3 className="text-sm capitalize">{service.capability}</h3><Readiness ready={service.ready} /></div>
            <div className="min-w-0 text-sm">
              <p className="break-all font-mono text-xs">{service.model}</p>
              <p className="mt-1 break-all font-mono text-xs text-muted-foreground">{service.base_url}</p>
              <p className="mt-1 text-xs text-muted-foreground">{service.purpose}</p>
              {service.problem && <Problem>{service.problem}</Problem>}
            </div>
          </div>)}
          {!doctor.services.length && <p className="py-4 text-sm text-muted-foreground">Model server status is unavailable. Check the configuration details above.</p>}
          {doctor.vectors && <div className="py-4 text-sm"><div className="flex items-center gap-3"><h3>Stored vectors</h3><Readiness ready={doctor.vectors.ready} /></div><p className="mt-1 break-all font-mono text-xs text-muted-foreground">{doctor.vectors.path}</p>{doctor.vectors.problem && <Problem>{doctor.vectors.problem}</Problem>}</div>}
        </div>
        <dl className="divide-y divide-border text-sm">{doctor.gates.map(gate => <Detail key={gate.gate} label={gate.gate}><Readiness ready={gate.ready} /><p className="mt-1 text-xs text-muted-foreground">{gate.ready ? 'Available' : 'Blocked'}: {gate.blocks}{gate.missing?.length ? `. Missing: ${gate.missing.join(', ')}` : ''}</p></Detail>)}</dl>
      </section>

      <section>
        <SectionRule>Corpus storage</SectionRule>
        <dl className="divide-y divide-border text-sm">
          <Detail label="Sources">{number(health.corpus.sources.length)}</Detail>
          <Detail label="Documents">{number(health.corpus.total)}</Detail>
          <Detail label="Current blob disk usage">{bytes(health.corpus.bytes)}</Detail>
          <Detail label="Missing blobs">{number(health.corpus.missing_blobs)}</Detail>
        </dl>
        <p className="mt-3 text-xs text-muted-foreground">Disk usage counts shared blobs once. It excludes historical blobs, extracted text and indexes. Total store size and free disk space are not measured.</p>
        {health.corpus.missing_blobs > 0 && <Problem>The logs reference missing blobs. Disk usage excludes their bytes; inspect the store before collecting again.</Problem>}
      </section>

      <section>
        <SectionRule>Collection</SectionRule>
        <dl className="divide-y divide-border text-sm">
          <Detail label="Collection lane">{health.running ? <><p>Held since {when(health.running.started_at)}</p><p className="mt-1 text-xs text-muted-foreground">{health.running.sources?.length ? health.running.sources.join(', ') : 'All enabled sources'} · {health.running.schedule ?? health.running.trigger} · PID {health.running.pid}</p><code className="mt-1 block break-all text-xs">{health.running.args}</code></> : 'Idle'}</Detail>
          <Detail label="Scheduler"><p className="capitalize">{health.scheduler.status}</p><p className="mt-1 text-xs text-muted-foreground">{health.scheduler.detail}</p></Detail>
        </dl>
      </section>
    </div>}
  </div>
}

function Readiness({ ready, good = 'Ready', bad = 'Unavailable' }: { ready: boolean; good?: string; bad?: string }) {
  return <span className={`text-xs font-medium ${ready ? 'text-muted-foreground' : 'text-destructive'}`}>{ready ? good : bad}</span>
}

function Detail({ label, children }: { label: string; children: ReactNode }) {
  return <div className="grid gap-1 py-3 sm:grid-cols-[12rem_minmax(0,1fr)]"><dt className="text-muted-foreground first-letter:uppercase">{label}</dt><dd className="min-w-0">{children}</dd></div>
}

function Problem({ children }: { children: ReactNode }) {
  return <p className="mt-2 whitespace-pre-wrap break-words text-xs text-destructive">{children}</p>
}
