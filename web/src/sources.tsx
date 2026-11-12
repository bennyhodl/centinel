import { useState, type ReactNode } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Link, useNavigate } from '@tanstack/react-router'
import { CircleCheck, CirclePlay, Ellipsis, ListChecks, Microscope, Pause, Play, Plus, ScrollText, Search, Settings, Trash2, Undo2, X } from 'lucide-react'
import { api, type JobPart, type JobState, type LaneHolder, type ListedSource, type NewSource, type RunAttempt, type SourceDetail, type SourceStage } from './api'
import { AddSource } from './add-source'
import { SourceSettings } from './source-settings'
import { InvestigateDialog, useInvestigate } from './investigate'
import { queries } from './queries'
import { Bar, LogLine, StopJobButton as StopButton, useJobs, useOpenJob } from './jobs'
import { activeJobs, activeParts, jobTitle, partStage } from './job-state'
import { number, plural, tail, when } from './format'
import { SourceIcon } from './host-icon'
import { behind, needsYou, runAction, standing, type Standing } from './source-state'
import { BackToSources, Empty, ErrorBox, IconButton, PageHeader, Pulse, SectionRule, Spinner } from './ui'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Input } from '@/components/ui/input'
import { Skeleton } from '@/components/ui/skeleton'

/** The run that holds the lane, whichever process started it: it can be watched and stopped. */
const useLaneJob = () => activeJobs(useJobs()).find(job => job.kind === 'run')

/** Keeps the list and every source's page current after something changed them. */
function useRefresh() {
  const client = useQueryClient()
  return () => { client.invalidateQueries({ queryKey: ['source'] }); client.invalidateQueries({ queryKey: ['sources'] }) }
}

// ── the list ───────────────────────────────────────────────────────────────

/** The two acquisition stages. A run that skips both fetches nothing and finishes what is stored. */
const ACQUIRE = ['discover', 'collect']

export function Sources() {
  const { data, error } = useQuery(queries.sources())
  const job = useLaneJob()
  const open = useOpenJob()
  const refresh = useRefresh()
  const [adding, setAdding] = useState<NewSource | null>(null)
  const [investigating, setInvestigating] = useState(false)
  const [find, setFind] = useState('')
  const [onlyNeedsYou, setOnlyNeedsYou] = useState(false)
  const [selected, setSelected] = useState<ReadonlySet<string>>(new Set())
  const [removing, setRemoving] = useState(false)

  const all = data?.sources ?? []
  const rows = all.filter(source => !source.removed).map(source => ({ source, standing: standing(source, job) }))
  const removed = all.filter(source => source.removed)
  const waiting = rows.filter(row => needsYou(row.standing))
  const words = find.trim().toLowerCase()
  const shown = (onlyNeedsYou ? waiting : rows).filter(({ source }) => !words || source.id.toLowerCase().includes(words) || source.target.toLowerCase().includes(words))
  // A selection is of sources still here; one removed or renamed elsewhere drops out of it.
  const picked = rows.filter(({ source }) => selected.has(source.id)).map(({ source }) => source)
  const lane = data?.running ?? null

  const pick = (id: string, on: boolean) => setSelected(current => { const next = new Set(current); if (on) next.add(id); else next.delete(id); return next })
  const clear = () => { setSelected(new Set()); setRemoving(false) }
  // The run's id is its job's id, so the drawer opens on it the moment it is accepted.
  const run = useMutation({
    mutationFn: ({ sources, skip }: { sources: string[]; skip?: string[] }) => api.runSources(sources, { skip }),
    onSuccess: ({ run_id }) => { open(run_id); clear(); refresh() },
  })
  const each = useMutation({
    mutationFn: (change: (id: string) => Promise<unknown>) => Promise.all(picked.map(source => change(source.id))),
    onSuccess: () => { clear(); refresh() },
  })
  const enabled = rows.filter(({ source }) => source.enabled).map(({ source }) => source.id)
  const allPaused = picked.length > 0 && picked.every(source => !source.enabled)
  const busy = Boolean(lane) || run.isPending
  const held = lane ? 'Another run holds the lane. It can start when that one finishes.' : undefined

  return <>
    <PageHeader title="Sources" detail={data ? `${plural(rows.length, 'source')} · ${plural(rows.reduce((sum, { source }) => sum + source.stages.collected, 0), 'document')}` : undefined}>
      <div className="flex w-full max-w-2xl items-center gap-2">
        <label className="relative min-w-0 flex-1">
          <Search className="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground" />
          <Input value={find} onChange={event => setFind(event.target.value)} placeholder="Find a source" aria-label="Find a source by name or address" className="pl-8" />
        </label>
        <button type="button" aria-pressed={onlyNeedsYou} onClick={() => setOnlyNeedsYou(!onlyNeedsYou)} className={`inline-flex h-9 shrink-0 items-center gap-2 rounded-full px-3 text-[13px] shadow-[inset_0_0_0_1px_var(--rule)] ${onlyNeedsYou ? 'bg-flame-soft text-flame-ink shadow-[inset_0_0_0_1px_var(--flame-line)]' : 'text-muted-foreground hover:text-foreground'}`}>
          <span className="size-1.5 rounded-full bg-flame" />Needs you<span className="tabular-nums">{waiting.length}</span>
        </button>
        <IconButton label="Investigate an address" variant="outline" onClick={() => setInvestigating(true)}><Microscope /></IconButton>
        <IconButton label="Add a source" variant="outline" onClick={() => setAdding({ id: '', site: '' })}><Plus /></IconButton>
        {job && <IconButton label="Open the log" variant="outline" onClick={() => open(job.id)}><ScrollText /></IconButton>}
        {/* The one button that starts everything is the one that stops it while it runs. */}
        {job?.stoppable
          ? <StopButton id={job.id} icon />
          : <IconButton label={held ?? 'Run every source'} disabled={busy || !enabled.length} onClick={() => run.mutate({ sources: enabled })}>{run.isPending ? <Spinner /> : <Play />}</IconButton>}
      </div>
    </PageHeader>
    <div className="grid max-w-5xl gap-6">
      {job ? <WorkingNow job={job} /> : lane && <LaneBanner holder={lane} />}
      {[error, run.error, each.error].map((error, i) => error && <ErrorBox key={i} error={error} />)}
      <section className="grid">
        {picked.length > 0
          ? <div className="flex h-11 items-center gap-2 rounded-lg bg-muted px-3 text-sm shadow-[inset_0_0_0_1px_var(--rule)]">
            <input type="checkbox" checked aria-label="Clear the selection" onChange={clear} className="size-3.5 accent-foreground" />
            <b className="font-semibold">{number(picked.length)} selected</b>
            <span className="flex-1" />
            {removing
              ? <>
                <span className="text-muted-foreground">Hide {picked.length === 1 ? 'it' : 'them'} and never run {picked.length === 1 ? 'it' : 'them'} again? What {picked.length === 1 ? 'it' : 'they'} collected stays.</span>
                <IconButton size="icon-sm" label={`Remove ${plural(picked.length, 'source')}`} variant="destructive" disabled={each.isPending} onClick={() => each.mutate(id => api.setSourceRemoved(id, true))}><Trash2 /></IconButton>
                <IconButton size="icon-sm" label="Keep them" variant="ghost" onClick={() => setRemoving(false)}><X /></IconButton>
              </>
              : <>
                <IconButton size="icon-sm" label={held ?? (picked.length === 1 ? 'Run it' : `Run these ${number(picked.length)} together`)} disabled={busy} onClick={() => run.mutate({ sources: picked.map(source => source.id) })}><Play /></IconButton>
                <IconButton size="icon-sm" label={held ?? 'Finish processing: read, index and embed what is collected. Nothing is fetched.'} variant="outline" disabled={busy} onClick={() => run.mutate({ sources: picked.map(source => source.id), skip: ACQUIRE })}><ListChecks /></IconButton>
                <IconButton size="icon-sm" label={allPaused ? 'Unpause' : 'Pause: a run over every source skips them'} variant="outline" disabled={each.isPending} onClick={() => each.mutate(id => api.setSourceEnabled(id, allPaused))}>{allPaused ? <CirclePlay /> : <Pause />}</IconButton>
                <IconButton size="icon-sm" label="Remove: hide them and never run them again" variant="outline" className="text-destructive" onClick={() => setRemoving(true)}><Trash2 /></IconButton>
                <IconButton size="icon-sm" label="Clear the selection" variant="ghost" onClick={clear}><X /></IconButton>
              </>}
          </div>
          : <ListHeader all={shown.length > 0 && <input type="checkbox" checked={false} aria-label="Select every source shown" onChange={() => setSelected(new Set(shown.map(({ source }) => source.id)))} className="size-3.5 accent-foreground" />} />}
        {shown.map(({ source, standing }) => <SourceRow key={source.id} source={source} standing={standing} selected={selected.has(source.id)} onSelect={on => pick(source.id, on)} />)}
        {data && !rows.length && <Empty>No sources yet. Investigate an address, then add it.</Empty>}
        {data && rows.length > 0 && !shown.length && <p className="py-6 text-sm text-muted-foreground">{onlyNeedsYou && !words ? 'Nothing needs you.' : 'No source matches.'}</p>}
      </section>
      {removed.length > 0 && <Removed sources={removed} />}
    </div>
    {adding && <AddSource initial={adding} onClose={() => setAdding(null)} />}
    {investigating && <InvestigateDialog onClose={() => setInvestigating(false)} />}
  </>
}

export function SourcesSkeleton() {
  return <>
    <PageHeader title="Sources" detail=" ">
      <div className="flex w-full max-w-2xl items-center gap-2"><Skeleton className="h-9 min-w-0 flex-1 rounded-md" /><Skeleton className="h-9 w-28 rounded-full" /><Button variant="outline" size="icon" disabled><Microscope /></Button><Button variant="outline" size="icon" disabled><Plus /></Button><Button size="icon" disabled><Play /></Button></div>
    </PageHeader>
    <div aria-busy className="grid max-w-5xl gap-6">
      <section className="grid">
        <ListHeader />
        {Array.from({ length: 6 }, (_, i) => <div key={i} className={ROW}>
          <span />
          <span className="flex min-w-0 items-center gap-3"><Skeleton className="size-5 rounded-[4px]" /><span className="grid min-w-0 gap-1"><b className="text-sm"><Skeleton mask="hillsborough-transportation" /></b><span className="text-xs"><Skeleton mask="planhillsborough.org" /></span></span></span>
          <span className="text-sm"><Skeleton mask="Interrupted before the first page" /></span>
          <span className="justify-self-end text-sm"><Skeleton mask="19,330" /></span>
          <span className="justify-self-end text-sm"><Skeleton mask="Oct 13" /></span>
        </div>)}
      </section>
    </div>
  </>
}

/** One row of the list and its header share these columns: tick, source, state, documents, collected. */
const ROW = 'grid grid-cols-[14px_minmax(0,1fr)_minmax(0,1.3fr)_96px_72px] items-center gap-4 border-b px-2 py-3'

function ListHeader({ all }: { all?: ReactNode }) {
  return <div className={`${ROW} h-11 py-0 text-xs text-muted-foreground`}>
    <span className="flex">{all}</span>
    <span>Source</span>
    <span>State</span>
    <span className="justify-self-end">Documents</span>
    <span className="justify-self-end">Collected</span>
  </div>
}

/** A row is a link to the source's page; its tick sits above the link, so ticking it does not navigate. */
function SourceRow({ source, standing, selected, onSelect }: { source: ListedSource; standing: Standing; selected: boolean; onSelect: (on: boolean) => void }) {
  return <div className={`relative ${ROW} hover:bg-hover ${selected ? 'bg-muted' : ''}`}>
    <input type="checkbox" checked={selected} aria-label={`Select ${source.id}`} onChange={event => onSelect(event.target.checked)} className="relative z-10 size-3.5 accent-foreground" />
    <span className="flex min-w-0 items-center gap-3">
      <SourceIcon source={source.id} size={20} />
      <span className="grid min-w-0 gap-0.5">
        <Link to="/sources/$id" params={{ id: source.id }} className={`truncate text-sm font-semibold after:absolute after:inset-0 ${source.enabled ? '' : 'text-muted-foreground'}`}>{source.id}</Link>
        <span className="truncate text-xs text-muted-foreground">{hostOf(source.target)}</span>
      </span>
    </span>
    <StandingLine standing={standing} />
    <span className="justify-self-end text-sm tabular-nums text-muted-foreground">{source.stages.collected ? number(source.stages.collected) : '—'}</span>
    <span className="justify-self-end text-sm text-muted-foreground">{source.last_run ? day(source.last_run.started_at) : '—'}</span>
  </div>
}

/** A source's mark and the few words beside it: a turning ring while it works, a check when it is done, a dot when it waits or is stuck. */
function StandingLine({ standing }: { standing: Standing }) {
  const mark = {
    working: <span className="grid size-4 shrink-0 place-items-center text-flame"><Spinner /></span>,
    done: <CircleCheck className="size-4 shrink-0 text-moss" aria-hidden />,
    waiting: <span className="grid size-4 shrink-0 place-items-center"><span className="size-1.5 rounded-full bg-flame" /></span>,
    blocked: <span className="grid size-4 shrink-0 place-items-center"><span className="size-1.5 rounded-full bg-destructive" /></span>,
    paused: <span className="grid size-4 shrink-0 place-items-center"><span className="size-1.5 rounded-full bg-stone" /></span>,
  }[standing.mark]
  const tone = standing.mark === 'working' ? 'text-flame-ink' : standing.mark === 'done' || standing.mark === 'paused' ? 'text-muted-foreground' : ''
  return <span className="flex min-w-0 items-center gap-2 text-sm">
    {mark}
    <span className={`truncate ${tone}`} title={standing.words}>{standing.words}</span>
    {standing.count && <span className="shrink-0 font-mono text-xs text-muted-foreground">{standing.count}</span>}
  </span>
}

/** An address without its scheme or trailing slash: `www.tampa.gov`, `www.youtube.com/@CityofTampa`. */
const hostOf = (target: string) => target.replace(/^https?:\/\//, '').replace(/\/$/, '')

/** A server timestamp as a short day: `Nov 6`. */
const day = (at: string) => new Date(at).toLocaleDateString('en-US', { month: 'short', day: 'numeric' })

/**
 * The run under way, source by source: what each is doing, how far through, and the
 * address it has in hand. After collection the run reads, indexes and embeds for every
 * source at once, and that is one card. A click opens the run's drawer.
 */
function WorkingNow({ job }: { job: JobState }) {
  const open = useOpenJob()
  const parts = activeParts(job)
  const cards: Array<{ key: string; name: ReactNode; at: Pick<JobPart, 'done' | 'total' | 'current' | 'message'> }> = !job.step && parts.length
    ? parts.map(part => ({ key: part.name, name: <><SourceIcon source={part.name} size={18} /><span className="truncate">{part.name}</span><span className="shrink-0 text-xs font-normal text-flame-ink">{partStage(part)}</span></>, at: part }))
    : [{ key: job.id, name: <span className="truncate">{jobTitle(job)}</span>, at: job }]
  return <section className="grid gap-2.5">
    <div className="flex items-center gap-2.5">
      <span className="text-flame"><Pulse /></span>
      <span className="text-[11px] font-semibold uppercase tracking-[0.08em] text-flame-ink">Working now</span>
      <span className="min-w-0 flex-1 truncate text-[13px] text-muted-foreground">{job.label}</span>
    </div>
    <div className="grid gap-2.5 sm:grid-cols-2 lg:grid-cols-3">
      {cards.map(({ key, name, at }) => <button key={key} type="button" onClick={() => open(job.id)} className="grid content-start gap-2 rounded-lg border bg-card p-3 text-left hover:bg-hover">
        <span className="flex min-w-0 items-center gap-2 text-sm font-semibold">{name}<span className="flex-1" /><span className="shrink-0 font-mono text-xs font-normal text-muted-foreground">{at.total ? `${number(at.done ?? 0)} / ${number(at.total)}` : ''}</span></span>
        <Bar job={at} thin />
        <span className="truncate font-mono text-[11px] text-muted-foreground">{at.current ? tail(at.current) : at.message ?? ' '}</span>
      </button>)}
    </div>
  </section>
}

/** What was removed: hidden from the list and never run, with everything collected kept. Each can be brought back. */
function Removed({ sources }: { sources: ListedSource[] }) {
  const refresh = useRefresh()
  const restore = useMutation({ mutationFn: (id: string) => api.setSourceRemoved(id, false), onSuccess: refresh })
  return <details className="group text-sm">
    <summary className="flex cursor-pointer list-none items-center gap-2 text-muted-foreground hover:text-foreground"><span className="transition-transform group-open:rotate-90">›</span>Removed<span className="tabular-nums">{sources.length}</span></summary>
    <p className="pt-2 pb-1 text-[13px] text-muted-foreground">Hidden and never run. Everything they collected is still in the archive and in search.</p>
    {sources.map(source => <div key={source.id} className="flex items-center gap-3 border-b py-2.5">
      <SourceIcon source={source.id} size={18} />
      <span className="min-w-0 flex-1 truncate text-muted-foreground"><Link to="/sources/$id" params={{ id: source.id }} className="font-medium text-foreground hover:underline">{source.id}</Link> · {hostOf(source.target)} · {plural(source.stages.collected, 'document')}</span>
      <IconButton size="icon-sm" label={`Restore ${source.id}`} variant="outline" disabled={restore.isPending} onClick={() => restore.mutate(source.id)}><Undo2 /></IconButton>
    </div>)}
    {restore.error && <ErrorBox error={restore.error} />}
  </details>
}

/**
 * The run holding the store's one lane. A second run waits for it; a click while it holds
 * says so. Its job is on the board whichever process started it — the page, a schedule or
 * a `centinel run` in a terminal — under the run id, so it can be watched and stopped.
 */
function LaneBanner({ holder }: { holder: LaneHolder }) {
  const open = useOpenJob()
  const job = useJobs().find(job => !job.outcome && (holder.operation ? job.kind === holder.operation && job.label === holder.sources?.join(', ') : job.id === holder.started_at))
  return <div className="flex items-center gap-3 rounded-lg bg-flame-soft px-4 py-3 text-sm text-flame-ink shadow-[inset_0_0_0_1px_var(--flame-line)]">
    <span className="size-2 shrink-0 animate-pulse rounded-full bg-flame" />
    <span className="flex-1">{holder.operation ? `${holder.operation} is processing` : 'A run is collecting'} {holder.sources?.length ? holder.sources.join(', ') : 'every enabled source'}, started {when(holder.started_at)}{holder.schedule ? ` by the ${holder.schedule} schedule` : ''}. Another waits for it to finish.</span>
    {job && <IconButton size="icon-sm" label="Open the log" variant="outline" onClick={() => open(job.id)}><ScrollText /></IconButton>}
    {job?.stoppable && <StopButton id={job.id} icon />}
  </div>
}

// ── one source ───────────────────────────────────────────────────────────────

export function SourcePage({ id }: { id: string }) {
  const { data, error } = useQuery(queries.source(id))
  if (error) return <><BackToSources /><ErrorBox error={error} /></>
  if (!data) return <SourceSkeleton id={id} />
  return <SourceView id={id} detail={data} />
}

function SourceView({ id, detail }: { id: string; detail: SourceDetail }) {
  const [settings, setSettings] = useState(false)
  const navigate = useNavigate()
  const open = useOpenJob()
  const refresh = useRefresh()
  const { source, summary, definition } = detail
  const job = useLaneJob()
  const here = standing(source, job)
  const lane = detail.running
  const held = lane ? 'Another run holds the lane. It can start when that one finishes.' : undefined
  // The run's id is its job's id, so the drawer opens on it the moment it is accepted.
  const run = useMutation({ mutationFn: (skip: string[]) => api.runSource(id, { skip }), onSuccess: ({ run_id }) => { open(run_id); refresh() } })
  const stage = useMutation({ mutationFn: ({ stage, again }: { stage: SourceStage; again?: boolean }) => api.startSourceStage(id, stage, again), onSuccess: ({ id: job }) => { open(job); refresh() } })
  const enable = useMutation({ mutationFn: (enabled: boolean) => api.setSourceEnabled(id, enabled), onSuccess: refresh })
  const remove = useMutation({ mutationFn: (removed: boolean) => api.setSourceRemoved(id, removed), onSuccess: (_, removed) => { refresh(); if (removed) void navigate({ to: '/sources' }) } })
  const investigate = useInvestigate()
  const busy = Boolean(lane) || run.isPending || stage.isPending
  const { live = 0, gone = 0, blocked = 0, error = 0 } = source.liveness
  const notLive = gone + blocked + error

  return <>
    <BackToSources />
    <PageHeader eyebrow={<span className="inline-flex items-center gap-2"><SourceIcon source={id} size={16} />{source.kind === 'channel' ? 'Channel' : 'Website'} · {hostOf(source.target)}</span>} title={id}>
      <div className="flex shrink-0 flex-wrap gap-2">
        {source.removed
          ? <IconButton label="Restore it, as it was" disabled={remove.isPending} onClick={() => remove.mutate(false)}><Undo2 /></IconButton>
          : here.mark === 'working'
            ? job?.stoppable ? <StopButton id={job.id} icon /> : null
            : <IconButton label={held ?? (runAction(source) === 'resume' ? 'Resume where it stopped' : 'Run it')} disabled={busy} onClick={() => run.mutate([])}>{run.isPending ? <Spinner /> : <Play />}</IconButton>}
        <IconButton label="Settings" variant="outline" onClick={() => setSettings(true)}><Settings /></IconButton>
        <DropdownMenu>
          <DropdownMenuTrigger asChild><Button variant="outline" size="icon" aria-label="More"><Ellipsis /></Button></DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            <DropdownMenuItem disabled={busy || source.removed} onSelect={() => run.mutate(['embed'])}>Run without embedding</DropdownMenuItem>
            <DropdownMenuItem disabled={busy} onSelect={() => stage.mutate({ stage: 'extract', again: true })}>Read everything again</DropdownMenuItem>
            {source.kind === 'channel' && <DropdownMenuItem disabled={busy} onSelect={() => stage.mutate({ stage: 'transcribe' })}>Transcribe recordings</DropdownMenuItem>}
            <DropdownMenuItem disabled={source.kind !== 'site' || investigate.isPending} onSelect={() => investigate.mutate(source.target)}>Investigate it again</DropdownMenuItem>
            <DropdownMenuSeparator />
            {!source.removed && <DropdownMenuItem disabled={enable.isPending} onSelect={() => enable.mutate(!source.enabled)}>{source.enabled ? 'Pause' : 'Unpause'}</DropdownMenuItem>}
            {!source.removed && <DropdownMenuItem variant="destructive" disabled={remove.isPending} onSelect={() => remove.mutate(true)}>Remove</DropdownMenuItem>}
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
    </PageHeader>
    <div className="grid max-w-5xl gap-9">
      <div className="-mt-3 grid gap-3">
        {source.removed
          ? <p className="text-sm text-muted-foreground">Removed: hidden from the list and never run. Everything it collected is still in the archive. Restore brings it back as it was.</p>
          : <StandingLine standing={here.mark === 'paused' ? { mark: 'paused', words: 'Paused: a run over every source skips it. Run starts it anyway.' } : here} />}
        {lane && here.mark !== 'working' && <LaneBanner holder={lane} />}
        {[run.error, stage.error, enable.error, remove.error, investigate.error].map((error, i) => error && <ErrorBox key={i} error={error} />)}
      </div>

      <section className="grid">
        <SectionRule aside={source.last_run ? `collected ${when(source.last_run.started_at)}` : undefined}>Where it stands</SectionRule>
        <Stand label="Collected" of={source.resources} got={live + notLive} main={source.resources ? `${number(live)} of ${plural(source.resources, 'address', 'addresses')} live` : 'Nothing yet'}
          detail={[gone && `${number(gone)} gone`, blocked && `${number(blocked)} blocked`, error && `${number(error)} failing`, source.resources > live + notLive && `${number(source.resources - live - notLive)} not collected yet`]} />
        <StagesOf source={source} busy={busy} held={held} pending={stage.isPending ? stage.variables.stage : undefined} onStart={which => stage.mutate({ stage: which })} />
      </section>

      <section className="grid">
        <SectionRule aside={detail.runs.length ? `last ${detail.runs.length}` : undefined}>Runs</SectionRule>
        {detail.runs.map(attempt => <RunRow key={attempt.run_id} attempt={attempt} />)}
        {!detail.runs.length && <p className="py-3 text-sm text-muted-foreground">No run of this source is in the journal yet.</p>}
      </section>

      <section className="grid">
        <More title="How it is collected" aside={definition.strategy ? `${definition.strategy}, pinned` : summary?.last_discovery?.method}>
          <Facts rows={[
            ['Address', source.target],
            ['Method', definition.strategy ? `${definition.strategy} (pinned)` : 'asked on every run'],
            ['Last discovery', summary?.last_discovery ? `${when(summary.last_discovery.at)} · ${summary.last_discovery.method} · ${plural(summary.last_discovery.addresses, 'address', 'addresses')}` : 'never'],
            ['Collected', summary ? plural(summary.observations, 'observation') : '—'],
            ...(definition.rps ? [['Requests a second', String(definition.rps)] as [string, string]] : []),
            ...(definition.matches?.length ? [['Only addresses containing', definition.matches.join(', ')] as [string, string]] : []),
            ...(source.kind === 'channel' ? [
              ['Language', definition.lang ?? `default (${detail.settings_options.defaults.lang})`] as [string, string],
              ['Audio fallback', definition.audio_if_no_captions === false ? 'never' : 'when captions are missing'] as [string, string],
            ] : []),
          ]} />
        </More>
        {summary?.problems.length ? <More title={`${plural(summary.problems.length + (summary.problems_truncated ?? 0), 'address', 'addresses')} not live`}>
          {summary.problems.map(problem => <div key={problem.natural_key} className="grid gap-0.5 border-t py-2.5">
            <span className="flex items-baseline gap-2"><Badge variant="outline">{problem.state}</Badge><span className="min-w-0 truncate font-mono text-[13px]">{problem.natural_key}</span></span>
            <span className="text-xs text-muted-foreground">{plural(problem.consecutive_failures, 'failure')} since {when(problem.since)}{problem.detail ? ` · ${problem.detail}` : ''}</span>
          </div>)}
        </More> : null}
        <More title="Links that leave this site"><div className="border-t py-2.5 text-sm"><CrumbsFact id={id} /></div></More>
      </section>

      {here.mark === 'working' && job && <RunStrip job={job} part={job.parts?.find(part => part.name === id)} />}
    </div>
    {settings && <SourceSettings definition={definition} options={detail.settings_options} onClose={() => setSettings(false)} />}
  </>
}

export function SourceSkeleton({ id }: { id?: string }) {
  return <>
    <BackToSources />
    <PageHeader eyebrow="Website" title={id ?? '…'}>
      <div className="flex shrink-0 gap-2"><Button size="icon" disabled><Play /></Button><Button variant="outline" size="icon" disabled><Settings /></Button><Button variant="outline" size="icon" disabled><Ellipsis /></Button></div>
    </PageHeader>
    <div aria-busy className="grid max-w-5xl gap-9">
      <section className="grid"><SectionRule>Where it stands</SectionRule>{Array.from({ length: 4 }, (_, i) => <Skeleton key={i} className="my-2.5 h-6" />)}</section>
      <section className="grid"><SectionRule>Runs</SectionRule>{Array.from({ length: 3 }, (_, i) => <Skeleton key={i} className="my-2.5 h-6" />)}</section>
    </div>
  </>
}

/**
 * The three stages after collection, each against the one before it: how many documents
 * got this far, how many wait, and a button on the stage that has work left. Only that
 * one: a stage ahead of it has nothing to do until it runs.
 */
function StagesOf({ source, busy, held, pending, onStart }: { source: ListedSource; busy: boolean; held?: string; pending?: SourceStage; onStart: (stage: SourceStage) => void }) {
  const { stages } = source
  const left = behind(stages)
  const next: SourceStage | undefined = left.read ? 'extract' : left.indexed ? 'index' : left.embedded || (stages.embedded === undefined && stages.indexed) ? 'embed' : undefined
  const start = (stage: SourceStage, label: string) => next === stage && !source.removed && <IconButton size="icon-sm" label={held ?? label} variant={stages.embedded === undefined && stage === 'embed' ? 'outline' : 'default'} disabled={busy} onClick={() => onStart(stage)}>{pending === stage ? <Spinner /> : <Play />}</IconButton>
  return <>
    <Stand label="Read" of={stages.collected} got={stages.read + stages.unreadable} main={plural(stages.read, 'document')}
      detail={[stages.unreadable && `${number(stages.unreadable)} could not be read`, left.read && `${number(left.read)} waiting`]} action={start('extract', 'Read the rest')} />
    <Stand label="Indexed" of={stages.read} got={stages.indexed} main={plural(stages.indexed, 'document')}
      detail={[stages.indexed > 0 && 'searchable by keyword', left.indexed && `${number(left.indexed)} waiting`]} action={start('index', 'Index the rest')} />
    <Stand label="Embedded" of={stages.indexed} got={stages.embedded ?? 0} main={stages.embedded === undefined ? 'Nothing in this archive is embedded yet' : plural(stages.embedded, 'document')}
      detail={[stages.embedded ? 'searchable by meaning' : false, left.embedded && `${number(left.embedded)} waiting`]} action={start('embed', stages.embedded ? 'Embed the rest' : 'Embed')} />
  </>
}

/** One stage of a source: a bar for how much of the stage before it got this far, what that is in words, and what is missing. */
function Stand({ label, of, got, main, detail, action }: { label: string; of: number; got: number; main: string; detail: Array<string | false | 0>; action?: ReactNode }) {
  const share = of ? Math.min(1, got / of) : 0
  const said = detail.filter(Boolean).join(' · ')
  return <div className="grid min-h-12 grid-cols-[88px_140px_minmax(0,1fr)_auto] items-center gap-4 border-b py-2">
    <span className="text-[11px] font-semibold uppercase tracking-[0.08em] text-muted-foreground">{label}</span>
    <span className="block h-1 rounded-full bg-track"><i className={`block h-full rounded-full ${share < 1 && of ? 'bg-flame' : 'bg-foreground'}`} style={{ width: `${share * 100}%` }} /></span>
    <span className="truncate text-sm" title={said}><b className="font-semibold">{main}</b>{said && <span className="text-muted-foreground"> · {said}</span>}</span>
    {action || <span />}
  </div>
}

/** A part of the page that stays shut until asked for. */
function More({ title, aside, children }: { title: string; aside?: string; children: ReactNode }) {
  return <details className="group border-b">
    <summary className="flex cursor-pointer list-none items-center gap-2.5 py-2.5 text-sm text-muted-foreground hover:text-foreground"><span className="w-2 transition-transform group-open:rotate-90">›</span>{title}{aside && <span className="text-xs">· {aside}</span>}</summary>
    <div className="pb-3 pl-[18px]">{children}</div>
  </details>
}

const outcomeDot: Record<RunAttempt['outcome'], string> = { ok: 'bg-moss', partial: 'bg-flame', failed: 'bg-destructive', skipped: 'bg-stone', interrupted: 'bg-flame' }

/** One run in a line: when, how it ended, and what it added or lost. */
function RunRow({ attempt }: { attempt: RunAttempt }) {
  const added = attempt.added
  const lost = attempt.subtracted
  const moved = [added.documents && `${number(added.documents)} new`, lost.gone && `${number(lost.gone)} gone`, lost.blocked && `${number(lost.blocked)} blocked`, lost.errored && `${number(lost.errored)} errors`].filter(Boolean).join(' · ')
  return <div className="grid grid-cols-[8px_128px_minmax(0,1fr)_auto] items-center gap-3 border-b py-2.5 text-sm">
    <span className={`size-1.5 rounded-full ${outcomeDot[attempt.outcome]}`} />
    <span>{when(attempt.started_at)}</span>
    <span className="truncate text-muted-foreground" title={attempt.detail}>{attempt.outcome === 'interrupted' ? 'stopped' : attempt.outcome === 'ok' ? 'finished' : attempt.outcome} · {moved || attempt.detail || 'nothing new'}</span>
    <span className="text-xs text-muted-foreground">{attempt.schedule ? `${attempt.schedule} schedule` : attempt.trigger}</span>
  </div>
}

/** The stages a run goes through, in order, for the strip that follows one. */
const STAGES = ['discover', 'collect', 'extract', 'index', 'embed']

/**
 * This source's run as it happens, pinned to the bottom of its page: the stage it is in,
 * how far through, and the last lines of its log. In a run over several sources that is
 * this source's part of it until the stages the run does for all of them. The drawer has
 * the whole log.
 */
function RunStrip({ job, part }: { job: JobState; part?: JobPart }) {
  const open = useOpenJob()
  const at = part && !job.step ? part : job
  const stage = STAGES.indexOf((at.step ?? '').split(' · ').at(-1) ?? '')
  const lines = job.log.filter(line => (line.type === 'item' || line.type === 'step') && (at === job || line.part === part?.name)).slice(-3)
  return <section className="sticky bottom-4 z-10 grid gap-3 rounded-lg border bg-background p-4 shadow-lg">
    <div className="flex items-center gap-3">
      <span className="text-flame"><Pulse /></span>
      <b className="text-sm font-semibold">{jobTitle({ kind: job.kind, step: at.step })}</b>
      {at.total != null && <span className="font-mono text-xs text-muted-foreground">{number(at.done ?? 0)} / {number(at.total)}</span>}
      <span className="flex flex-1 justify-end gap-1.5 text-xs">
        {STAGES.map((name, i) => <span key={name} className={`rounded-md px-2 py-0.5 ${i === stage ? 'bg-flame-soft text-flame-ink' : i < stage ? 'text-foreground' : 'text-muted-foreground'}`}>{name}</span>)}
      </span>
    </div>
    <Bar job={at} thin />
    <div className="rounded-md bg-terminal py-2 font-mono text-xs leading-[22px]">
      {lines.map(line => <LogLine key={line.seq} line={line} />)}
      {!lines.length && <div className="px-5 text-[#B9AE98]">Waiting for the first page…</div>}
    </div>
    <div className="flex justify-end gap-2">
      <IconButton size="icon-sm" label="Open the full log" variant="outline" onClick={() => open(job.id)}><ScrollText /></IconButton>
      {job.stoppable && <StopButton id={job.id} icon />}
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
