import { useEffect, useState, type ReactNode } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Link } from '@tanstack/react-router'
import { Ellipsis, Play, Plus, Search, Square } from 'lucide-react'
import { api, type JobState, type LaneHolder, type ListedSource, type NewSource, type RunAttempt, type SourceDetail } from './api'
import { AddSource } from './add-source'
import { InvestigateDialog, useInvestigate } from './investigate'
import { queries } from './queries'
import { Bar, LogLine, useJobs, useOpenJob } from './jobs'
import { activeJobs, jobTitle } from './job-state'
import { number, plural, when } from './format'
import { SourceIcon } from './host-icon'
import { inTab, nextAction, runAction, sourceState, TABS, type SourceState, type Tab } from './source-state'
import { BackToSources, Empty, ErrorBox, PageHeader, Pulse, SectionRule, Spinner } from './ui'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Skeleton } from '@/components/ui/skeleton'
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs'

const listDetail = 'What this archive collects. Investigate an address before adding it, run a source when you want it fresh, and see what each one holds.'

// ── the list ───────────────────────────────────────────────────────────────

export function Sources() {
  const { data, error } = useQuery(queries.sources())
  const [adding, setAdding] = useState<NewSource | null>(null)
  const [investigating, setInvestigating] = useState(false)
  const [tab, setTab] = useState<Tab>('all')
  const rows = (data?.sources ?? []).map(source => ({ source, state: sourceState(source, data?.running) }))
  const shown = rows.filter(row => inTab(tab, row.state))
  return <>
    <PageHeader title="Sources" detail={listDetail}>
      <div className="flex shrink-0 gap-2">
        <Button variant="outline" onClick={() => setInvestigating(true)}><Search />Investigate an address</Button>
        <Button onClick={() => setAdding({ id: '', site: '' })}><Plus />Add a source</Button>
      </div>
    </PageHeader>
    <div className="grid max-w-5xl gap-10">
      {data?.running && <LaneBanner holder={data.running} />}
      <section className="grid">
        <Tabs value={tab} onValueChange={value => setTab(value as Tab)}>
          <TabsList variant="line">
            {TABS.map(([value, label]) => <TabsTrigger key={value} value={value}>{label}<span className="tabular-nums text-muted-foreground">{rows.filter(row => inTab(value, row.state)).length}</span></TabsTrigger>)}
          </TabsList>
        </Tabs>
        <ListHeader />
        {error && <ErrorBox error={error} />}
        {shown.map(({ source, state }) => <SourceRow key={source.id} source={source} state={state} lane={data?.running ?? null} />)}
        {data && !rows.length && <Empty>No sources yet. Investigate an address, then add it.</Empty>}
        {data && rows.length > 0 && !shown.length && <p className="py-6 text-sm text-muted-foreground">No source is here.</p>}
      </section>
    </div>
    {adding && <AddSource initial={adding} onClose={() => setAdding(null)} />}
    {investigating && <InvestigateDialog onClose={() => setInvestigating(false)} />}
  </>
}

export function SourcesSkeleton() {
  return <>
    <PageHeader title="Sources" detail={listDetail}>
      <div className="flex shrink-0 gap-2"><Button variant="outline" disabled><Search />Investigate an address</Button><Button disabled><Plus />Add a source</Button></div>
    </PageHeader>
    <div aria-busy className="grid max-w-5xl gap-10">
      <section className="grid">
        <div className="flex h-9 gap-4">{TABS.map(([value, label]) => <span key={value} className="py-2 text-sm"><Skeleton mask={`${label} 16`} /></span>)}</div>
        <ListHeader />
        {Array.from({ length: 6 }, (_, i) => <div key={i} className="flex items-center gap-6 border-b py-3.5">
          <Skeleton className="size-5 rounded-[4px]" />
          <span className="grid min-w-0 flex-1 gap-1"><b className="text-sm"><Skeleton mask="hillsborough-transportation" /></b><span className="text-xs"><Skeleton mask="site · https://www.hillsboroughcounty.org" /></span></span>
          <span className="grid w-48 gap-1"><Skeleton className="h-1.5 rounded-full" /><span className="text-xs"><Skeleton mask="19,330 live of 19,400" /></span></span>
          <span className="grid w-36 gap-1 text-sm"><Skeleton mask="2026-10-13 12:00" /><span className="text-xs"><Skeleton mask="nothing new" /></span></span>
          <Skeleton className="h-8 w-20 rounded-md" />
        </div>)}
      </section>
    </div>
  </>
}

function ListHeader() {
  return <div className="flex gap-6 border-b pt-4 pb-2 text-xs text-muted-foreground">
    <span className="flex-1 pl-11">Source</span>
    <span className="w-48">Holds</span>
    <span className="w-36">Last run</span>
    <span className="w-24" />
  </div>
}

/** A row is a link to the source's page; its one button sits above the link, so a click on it does not navigate. */
function SourceRow({ source, state, lane }: { source: ListedSource; state: SourceState; lane: LaneHolder | null }) {
  return <div className="relative flex items-center gap-6 border-b py-3.5 hover:bg-hover">
    <SourceIcon source={source.id} size={20} />
    <span className="grid min-w-0 flex-1 gap-0.5">
      <Link to="/sources/$id" params={{ id: source.id }} className={`truncate text-sm font-semibold after:absolute after:inset-0 ${source.enabled ? '' : 'text-muted-foreground'}`}>{source.id}</Link>
      <span className="truncate text-xs text-muted-foreground">{source.kind} · {source.target}</span>
    </span>
    <Holds resources={source.resources} liveness={source.liveness} className="w-48" />
    <span className="grid w-36 gap-0.5 text-sm">
      {source.last_run
        ? <><span className={outcomeTone[source.last_run.outcome]}>{when(source.last_run.started_at)}</span><span className="text-xs text-muted-foreground">{source.last_run.outcome === 'ok' ? source.last_run.documents ? `${number(source.last_run.documents)} new documents` : 'nothing new' : source.last_run.outcome}</span></>
        : <span className="text-muted-foreground">never</span>}
    </span>
    <span className="relative z-10 flex w-24 justify-end"><SourceAction source={source} action={nextAction(state, source)} lane={lane} /></span>
  </div>
}

/** How much of what a source knows is live: one bar, collected states in order, the not-yet-collected rest as track. */
function Holds({ resources, liveness, className = '' }: { resources: number; liveness: Record<string, number>; className?: string }) {
  const { live = 0, gone = 0, blocked = 0, error = 0 } = liveness
  const parts: Array<[number, string, string]> = [
    [live, 'bg-foreground', 'live'],
    [gone, 'bg-muted-foreground/50', 'gone'],
    [blocked + error, 'bg-flame', 'blocked or failing'],
    [Math.max(0, resources - live - gone - blocked - error), 'bg-track', 'not collected yet'],
  ]
  const said = parts.filter(([n]) => n).map(([n, , label]) => `${number(n)} ${label}`).join(' · ')
  return <span className={`grid gap-1 ${className}`} title={said}>
    <span className="flex h-1.5 overflow-hidden rounded-full bg-track">
      {resources > 0 && parts.map(([n, tone, label]) => n > 0 && <i key={label} className={`block h-full ${tone}`} style={{ width: `${n / resources * 100}%` }} />)}
    </span>
    <span className="text-xs tabular-nums text-muted-foreground">{resources ? `${number(live)} live of ${number(resources)}` : 'nothing yet'}</span>
  </span>
}

/** Starts, stops, resumes or enables one source, and keeps the list and its page current. */
function useSourceControls(id: string) {
  const client = useQueryClient()
  const open = useOpenJob()
  const refresh = () => { client.invalidateQueries({ queryKey: ['source', id] }); client.invalidateQueries({ queryKey: ['sources'] }) }
  // The run's id is its job's id, so the drawer opens on it the moment it is accepted.
  const run = useMutation({ mutationFn: (skip: string[]) => api.runSource(id, { skip }), onSuccess: ({ run_id }) => { open(run_id); refresh() } })
  const enable = useMutation({ mutationFn: (enabled: boolean) => api.setSourceEnabled(id, enabled), onSuccess: refresh })
  return { run, enable }
}

/** The run this server started that holds the lane, if one does: it can be watched and stopped. */
const useLaneJob = () => activeJobs(useJobs()).find(job => job.kind === 'run')

function SourceAction({ source, action, lane }: { source: Pick<ListedSource, 'id'>; action: ReturnType<typeof nextAction>; lane: LaneHolder | null }) {
  const job = useLaneJob()
  const { run, enable } = useSourceControls(source.id)
  const error = run.error ?? enable.error
  switch (action) {
    case 'stop': return job?.stoppable ? <StopButton id={job.id} /> : <Badge className="bg-flame-soft text-flame-ink">Collecting</Badge>
    case 'enable': return <Button size="sm" variant="outline" disabled={enable.isPending} title={error?.message} onClick={() => enable.mutate(true)}>Enable</Button>
    default: return <Button size="sm" variant={action === 'resume' ? 'default' : 'outline'} disabled={Boolean(lane) || run.isPending} title={lane ? 'Another run holds the lane' : error?.message} onClick={() => run.mutate([])}>
      {run.isPending ? <Spinner /> : <Play />}{action === 'resume' ? 'Resume' : 'Run'}
    </Button>
  }
}

/**
 * The run holding the store's one lane. A second run waits for it; a click while it holds
 * says so. The job is this server's when the run started here (its id is the run id), and
 * then it can be watched and stopped; a `centinel run` in another terminal holds the lane
 * with no job to show.
 */
function LaneBanner({ holder }: { holder: LaneHolder }) {
  const open = useOpenJob()
  const job = useLaneJob()
  return <div className="flex items-center gap-3 rounded-lg bg-flame-soft px-4 py-3 text-sm text-flame-ink shadow-[inset_0_0_0_1px_var(--flame-line)]">
    <span className="size-2 shrink-0 animate-pulse rounded-full bg-flame" />
    <span className="flex-1">A run is collecting {holder.sources?.length ? holder.sources.join(', ') : 'every enabled source'}, started {when(holder.started_at)}{holder.schedule ? ` by the ${holder.schedule} schedule` : ''}. Another waits for it to finish.</span>
    {job && <Button size="sm" variant="outline" onClick={() => open(job.id)}>Watch</Button>}
    {job?.stoppable && <StopButton id={job.id} />}
  </div>
}

/** Asks a job to stop. It ends at its next item, so the button says so until the job is gone. */
function StopButton({ id }: { id: string }) {
  const stop = useMutation({ mutationFn: () => api.stopJob(id) })
  const stopping = stop.isPending || stop.isSuccess
  return <Button size="sm" variant="outline" disabled={stopping} onClick={() => stop.mutate()} title={stop.error?.message}>
    {stopping ? <Spinner /> : <Square />}{stopping ? 'Stopping' : 'Stop'}
  </Button>
}

// ── one source ───────────────────────────────────────────────────────────────

export function SourcePage({ id }: { id: string }) {
  const { data, error } = useQuery(queries.source(id))
  if (error) return <><BackToSources /><ErrorBox error={error} /></>
  if (!data) return <SourceSkeleton id={id} />
  return <SourceView id={id} detail={data} />
}

function SourceView({ id, detail }: { id: string; detail: SourceDetail }) {
  const definition = detail.definition
  const enabled = definition.enabled !== false
  const target = definition.site ?? definition.channel ?? ''
  const summary = detail.summary
  const source = { id, enabled, resources: summary?.resources ?? 0, liveness: summary?.liveness ?? {}, last_run: detail.runs[0] }
  const state = sourceState(source, detail.running)
  const job = useLaneJob()
  const { run, enable } = useSourceControls(id)
  const investigate = useInvestigate()
  const { live = 0, gone = 0, blocked = 0, error = 0 } = source.liveness
  const last = detail.runs[0]
  return <>
    <BackToSources />
    <PageHeader eyebrow={<span className="inline-flex items-center gap-2"><SourceIcon source={id} size={16} />{definition.channel ? 'Channel' : 'Website'}</span>} title={id} detail={target}>
      <div className="flex shrink-0 flex-wrap gap-2">
        <Button variant="outline" disabled={enable.isPending} onClick={() => enable.mutate(!enabled)}>{enabled ? 'Disable' : 'Enable'}</Button>
        <SourceAction source={source} action={state === 'collecting' ? 'stop' : runAction(source)} lane={detail.running} />
        <DropdownMenu>
          <DropdownMenuTrigger asChild><Button variant="outline" size="icon" aria-label="More"><Ellipsis /></Button></DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            <DropdownMenuItem disabled={Boolean(detail.running) || run.isPending} onSelect={() => run.mutate(['embed'])}>Run without embedding</DropdownMenuItem>
            <DropdownMenuItem disabled={!definition.site || investigate.isPending} onSelect={() => investigate.mutate(target)}>Investigate it again</DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
    </PageHeader>
    <div className="grid max-w-5xl gap-10">
      {detail.running && !(state === 'collecting' && job) && <LaneBanner holder={detail.running} />}
      {[run.error, enable.error, investigate.error].map((error, i) => error && <ErrorBox key={i} error={error} />)}
      {!enabled && <p className="text-sm text-muted-foreground">Disabled: a run over every source skips it. Run starts it anyway.</p>}

      <section className="grid gap-4">
        <div className="grid grid-cols-4 border-y">
          <Figure label="Addresses" value={number(source.resources)} />
          <Figure label="Live" value={number(live)} />
          <Figure label="Not live" value={number(gone + blocked + error)} detail={[gone && `${number(gone)} gone`, blocked && `${number(blocked)} blocked`, error && `${number(error)} failing`].filter(Boolean).join(' · ')} />
          <Figure label="Last run" value={last ? when(last.started_at) : 'never'} detail={last && (last.schedule ? `${last.outcome} · ${last.schedule} schedule` : `${last.outcome} · ${last.trigger}`)} />
        </div>
        <Holds resources={source.resources} liveness={source.liveness} />
      </section>

      <section className="grid gap-3">
        <SectionRule>How it is collected</SectionRule>
        <Facts rows={[
          ['Method', definition.strategy ? `${definition.strategy} (pinned)` : 'asked on every run'],
          ['Last discovery', summary?.last_discovery ? `${when(summary.last_discovery.at)} · ${summary.last_discovery.method} · ${plural(summary.last_discovery.addresses, 'address', 'addresses')}` : 'never'],
          ['Collected', summary ? plural(summary.observations, 'observation') : '—'],
          ['Links off-site', <CrumbsFact key="crumbs" id={id} />],
          ...(definition.rps ? [['Requests a second', String(definition.rps)] as [string, string]] : []),
          ...(definition.matches?.length ? [['Only addresses containing', definition.matches.join(', ')] as [string, string]] : []),
        ]} />
      </section>

      {summary?.problems.length ? <section className="grid">
        <SectionRule aside={plural(summary.problems.length + (summary.problems_truncated ?? 0), 'address', 'addresses')}>Not live</SectionRule>
        {summary.problems.map(problem => <div key={problem.natural_key} className="grid gap-0.5 border-b py-2.5">
          <span className="flex items-baseline gap-2"><Badge variant="outline">{problem.state}</Badge><span className="min-w-0 truncate font-mono text-[13px]">{problem.natural_key}</span></span>
          <span className="text-xs text-muted-foreground">{plural(problem.consecutive_failures, 'failure')} since {when(problem.since)}{problem.detail ? ` · ${problem.detail}` : ''}</span>
        </div>)}
      </section> : null}

      <section className="grid">
        <SectionRule aside={detail.runs.length ? `last ${detail.runs.length}` : undefined}>Runs</SectionRule>
        {detail.runs.map(attempt => <RunRow key={attempt.run_id} attempt={attempt} />)}
        {!detail.runs.length && <p className="py-4 text-sm text-muted-foreground">No run of this source is in the journal yet.</p>}
      </section>

      {state === 'collecting' && job && <RunStrip job={job} />}
    </div>
  </>
}

export function SourceSkeleton({ id }: { id?: string }) {
  return <>
    <BackToSources />
    <PageHeader eyebrow="Website" title={id ?? '…'}>
      <div className="flex shrink-0 gap-2"><Button variant="outline" disabled>Disable</Button><Button size="sm" variant="outline" disabled><Play />Run</Button><Button variant="outline" size="icon" disabled><Ellipsis /></Button></div>
    </PageHeader>
    <div aria-busy className="grid max-w-5xl gap-10">
      <section className="grid gap-4"><Skeleton className="h-[74px] rounded-none" /><Skeleton className="h-6" /></section>
      <section className="grid gap-3"><SectionRule>How it is collected</SectionRule><Skeleton className="h-40 rounded-md" /></section>
      <section className="grid"><SectionRule>Runs</SectionRule>{Array.from({ length: 3 }, (_, i) => <Skeleton key={i} className="my-2.5 h-9" />)}</section>
    </div>
  </>
}

function Figure({ label, value, detail }: { label: string; value: string; detail?: string | false }) {
  return <span className="grid content-start gap-0.5 border-l py-3 pl-4 first:border-l-0 first:pl-0">
    <span className="text-xs text-muted-foreground">{label}</span>
    <b className="text-lg leading-6 font-semibold tabular-nums">{value}</b>
    {detail && <span className="truncate text-xs text-muted-foreground">{detail}</span>}
  </span>
}

const outcomeTone: Record<RunAttempt['outcome'], string> = { ok: '', partial: 'text-flame-ink', failed: 'text-destructive', skipped: 'text-muted-foreground', interrupted: 'text-muted-foreground' }

function RunRow({ attempt }: { attempt: RunAttempt }) {
  const added = attempt.added
  const lost = attempt.subtracted
  return <div className="flex items-center gap-6 border-b py-3">
    <span className="grid w-40 shrink-0 gap-0.5"><b className="text-sm font-semibold">{when(attempt.started_at)}</b><span className="text-xs text-muted-foreground">{attempt.schedule ? `${attempt.schedule} schedule` : attempt.trigger}</span></span>
    <span className={`w-24 shrink-0 text-sm ${outcomeTone[attempt.outcome]}`}>{attempt.outcome === 'interrupted' ? 'stopped' : attempt.outcome}</span>
    <span className="min-w-0 flex-1 truncate text-[13px] text-muted-foreground" title={attempt.detail}>
      {[added.documents && `${number(added.documents)} new documents`, added.chunks && `${number(added.chunks)} chunks`, lost.gone && `${number(lost.gone)} gone`, lost.blocked && `${number(lost.blocked)} blocked`, lost.errored && `${number(lost.errored)} errors`].filter(Boolean).join(' · ') || attempt.detail || 'nothing new'}
    </span>
  </div>
}

/** The stages a run goes through, in order, for the strip that follows one. */
const STAGES = ['discover', 'collect', 'extract', 'index', 'embed']

/**
 * This source's run as it happens, pinned to the bottom of its page: the stage it is in,
 * how far through, and the last lines of its log. The drawer has the whole log.
 */
function RunStrip({ job }: { job: JobState }) {
  const open = useOpenJob()
  const stage = STAGES.indexOf((job.step ?? '').split(' · ').at(-1) ?? '')
  const lines = job.log.filter(line => line.type === 'item' || line.type === 'step').slice(-3)
  return <section className="sticky bottom-4 z-10 grid gap-3 rounded-lg border bg-background p-4 shadow-lg">
    <div className="flex items-center gap-3">
      <span className="text-flame"><Pulse /></span>
      <b className="text-sm font-semibold">{jobTitle(job)}</b>
      {job.total != null && <span className="font-mono text-xs text-muted-foreground">{number(job.done ?? 0)} / {number(job.total)}</span>}
      <span className="flex flex-1 justify-end gap-1.5 text-xs">
        {STAGES.map((name, i) => <span key={name} className={`rounded-md px-2 py-0.5 ${i === stage ? 'bg-flame-soft text-flame-ink' : i < stage ? 'text-foreground' : 'text-muted-foreground'}`}>{name}</span>)}
      </span>
    </div>
    <Bar job={job} thin />
    <div className="rounded-md bg-terminal py-2 font-mono text-xs leading-[22px]">
      {lines.map(line => <LogLine key={line.seq} line={line} />)}
      {!lines.length && <div className="px-5 text-[#B9AE98]">Waiting for the first page…</div>}
    </div>
    <div className="flex justify-end gap-2">
      <Button size="sm" variant="outline" onClick={() => open(job.id)}>Open full log</Button>
      {job.stoppable && <StopButton id={job.id} />}
    </div>
  </section>
}

/**
 * How many hosts this source links to, out of the ledger `collect` writes. A source collected
 * before the ledger existed has pages with no row; **Record** reads their blobs once, as a
 * job, and writes the rows down; the ledger is read again when it ends.
 */
function CrumbsFact({ id }: { id: string }) {
  const openJob = useOpenJob()
  const crumbs = useQuery(queries.sourceCrumbs(id))
  const rescan = useMutation({ mutationFn: () => api.rescanCrumbs(id), onSuccess: ({ id }) => openJob(id) })
  const job = useJobs().find(candidate => candidate.id === rescan.data?.id)
  const recording = rescan.isPending || Boolean(job && !job.outcome)
  if (crumbs.error) return <span className="text-destructive">{crumbs.error.message}</span>
  if (!crumbs.data) return <Skeleton mask="12 hosts, from 1,005 pages" />
  const unrecorded = crumbs.data.unrecorded
  return <span className="flex flex-wrap items-center gap-3">
    <span>{plural(crumbs.data.crumbs.length, 'host')}, from {plural(crumbs.data.documents, 'page')}{unrecorded > 0 && ` · ${plural(unrecorded, 'page')} not yet read`}</span>
    {unrecorded > 0 && <Button size="sm" variant="outline" disabled={recording} title={rescan.error?.message} onClick={() => rescan.mutate()}>{recording && <Spinner />}{recording ? 'Recording' : 'Record'}</Button>}
  </span>
}

// ── shared ──────────────────────────────────────────────────────────────────

function Facts({ rows }: { rows: Array<[string, ReactNode]> }) {
  return <dl className="grid grid-cols-[180px_minmax(0,1fr)] text-sm [&_dd]:border-t [&_dd]:py-2 [&_dd]:wrap-anywhere [&_dt]:border-t [&_dt]:py-2 [&_dt]:text-muted-foreground">
    {rows.map(([label, value]) => <Fact key={label} label={label} value={value} />)}
  </dl>
}

function Fact({ label, value }: { label: string; value: ReactNode }) {
  return <><dt>{label}</dt><dd>{value}</dd></>
}

