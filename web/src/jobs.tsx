import { createContext, useContext, useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react'
import { Link } from '@tanstack/react-router'
import { useQuery, useQueryClient, type QueryKey } from '@tanstack/react-query'
import { jobEventsUrl, type JobEvent, type JobItem, type JobState, type RunResult } from './api'
import { activeJobs, foldJob, jobTitle, succeeded } from './job-state'
import { clock, number, seconds, tail } from './format'
import { answerSummary } from './live'
import { queries } from './queries'
import { DocumentLink } from './ui'
import { Sheet, SheetContent, SheetDescription, SheetTitle } from '@/components/ui/sheet'
import { SidebarMenu, SidebarMenuButton, SidebarMenuItem } from '@/components/ui/sidebar'

/**
 * Keeps the `jobs` cache current from the server's job stream. Mounted once, in the shell:
 * the stream opens on a snapshot and then sends each change, so every page reading the
 * jobs re-renders as they move, with nothing polling. `EventSource` reconnects on its
 * own, and each reconnect opens on a fresh snapshot.
 */
export function useJobEvents(enabled = true) {
  const client = useQueryClient()
  useEffect(() => {
    if (!enabled) return
    const stream = new EventSource(jobEventsUrl)
    const key = queries.jobs().queryKey
    stream.addEventListener('snapshot', message => {
      client.setQueryData(key, (JSON.parse(message.data) as { jobs: JobState[] }).jobs)
    })
    stream.addEventListener('job', message => {
      const event = JSON.parse(message.data) as JobEvent
      client.setQueryData(key, jobs => foldJob(jobs ?? [], event))
      if (event.type !== 'finished') return
      const kind = client.getQueryData(key)?.find(job => job.id === event.job)?.kind
      for (const queryKey of (kind && ended[kind]?.(event.job)) || []) client.invalidateQueries({ queryKey })
    })
    // Every answer a running classifier writes is live in the corpus at once, so a page
    // showing the corpus is refreshed every few seconds while one scores.
    const refresh = setInterval(() => {
      if (activeJobs(client.getQueryData(key) ?? []).some(job => job.kind === 'classify')) client.invalidateQueries({ queryKey: ['corpus'] })
    }, CORPUS_REFRESH_MS)
    return () => { stream.close(); clearInterval(refresh) }
  }, [client, enabled])
}

/**
 * What a job changes by ending, by kind: the queries to read again. A classifier run has
 * changed the corpus; a collection run, finished or stopped, every source's state and the
 * links its pages carry; a crumbs rescan, the links.
 */
const ended: Record<string, (job: string) => QueryKey[]> = {
  classify: job => [['runs'], ['run', job], ['corpus']],
  run: () => [['sources'], ['source'], ['source-crumbs'], ['graph'], ['graph-host']],
  crumbs: () => [['source-crumbs'], ['graph'], ['graph-host']],
}

/** How often the corpus is re-read while a classifier scores. */
const CORPUS_REFRESH_MS = 5_000

/** Every job the server knows of: running first, in the order they started, then finished. */
export function useJobs() {
  return useQuery(queries.jobs()).data ?? []
}

const OpenJob = createContext<(id: string) => void>(() => {})

/** Opens one job in the drawer, from any page: a page that starts a job shows it this way. */
export const useOpenJob = () => useContext(OpenJob)

/** The one job drawer. Mounted once, in the shell, around everything that can open it. */
export function JobDrawerProvider({ children }: { children: ReactNode }) {
  const [id, setId] = useState('')
  return <OpenJob.Provider value={setId}>
    {children}
    {id && <JobDrawer id={id} onClose={() => setId('')} />}
  </OpenJob.Provider>
}

/**
 * What is running right now, from any page: what each job is doing and how far along. In
 * the collapsed rail, one mark that says something runs and opens the oldest of it.
 */
export function WorkingNow() {
  const open = useOpenJob()
  const running = activeJobs(useJobs())
  if (!running.length) return null
  const first = running[0]
  return <>
    <div className="grid gap-1 border-t pt-3 group-data-[collapsible=icon]:hidden">
      <span className="mb-1 inline-flex items-center gap-2 text-[11px] font-semibold uppercase tracking-[0.08em] text-flame-ink"><span className="size-1.5 rounded-full bg-flame shadow-[0_0_0_3px_var(--flame-halo)]" />Working now</span>
      {running.map(job => <button key={job.id} type="button" onClick={() => open(job.id)} className="grid gap-1 rounded-md px-1.5 py-1 text-left text-[13px] hover:bg-background">
        <span className="flex justify-between gap-2"><span className="truncate">{jobTitle(job)}</span><span className="shrink-0 font-mono text-xs text-muted-foreground">{count(job)}</span></span>
        <Bar job={job} thin />
        {job.current && <span className="truncate font-mono text-[11px] text-muted-foreground">{tail(job.current)}</span>}
      </button>)}
    </div>
    <SidebarMenu className="-mx-1 hidden group-data-[collapsible=icon]:flex">
      <SidebarMenuItem>
        <SidebarMenuButton onClick={() => open(first.id)} tooltip={[jobTitle(first), count(first), running.length > 1 && `${running.length - 1} more`].filter(Boolean).join(' · ')} className="h-9">
          <Ring job={first} /><span>Working now</span>
        </SidebarMenuButton>
      </SidebarMenuItem>
    </SidebarMenu>
  </>
}

/** A job's progress as a ring around the live dot, for the collapsed rail. */
function Ring({ job }: { job: JobState }) {
  const done = job.total ? (job.done ?? 0) / job.total : 0
  return <span className="relative grid size-4 shrink-0 place-items-center">
    <svg viewBox="0 0 16 16" className="absolute inset-0 -rotate-90">
      <circle cx="8" cy="8" r="7" fill="none" strokeWidth="2" className="stroke-track" />
      <circle cx="8" cy="8" r="7" fill="none" strokeWidth="2" pathLength={1} strokeDasharray={`${done} 1`} className="stroke-flame transition-[stroke-dasharray] duration-(--motion-progress) ease-out" />
    </svg>
    <span className="size-1.5 animate-pulse rounded-full bg-flame" />
  </span>
}

const count = (job: JobState) => job.total ? `${number(job.done ?? 0)}/${number(job.total)}` : ''

/** How far through its work list a job is. */
export function Bar({ job, thin }: { job: JobState; thin?: boolean }) {
  const width = job.total ? (job.done ?? 0) / job.total * 100 : 0
  return <span className={`block rounded-full bg-track ${thin ? 'h-1' : 'h-1.5'}`}>
    <i className="block h-full rounded-full bg-foreground transition-[width] duration-(--motion-progress) ease-out" style={{ width: `${job.outcome ? 100 : width}%` }} />
  </span>
}

const verdictTone: Record<JobItem['verdict'], string> = { ok: 'text-[#8FB58A]', warn: 'text-[#E8A65A]', missing: 'text-[#E8A65A]', fail: 'text-[#E57D6E]' }

/**
 * One job as it happens, line by line, on the server's clock: the stage it is in, the
 * item in hand, how far through its work list, and every page or document it finished,
 * failures in their own colour. A classifier run also reads its answers from the run, so
 * a scored line says what Jev decided.
 */
function JobDrawer({ id, onClose }: { id: string; onClose: () => void }) {
  const job = useJobs().find(candidate => candidate.id === id)
  const classify = job?.kind === 'classify'
  const detail = useQuery({ ...queries.run(id, { page: 1, page_size: 1 }, 2000), enabled: classify })
  const run = classify ? detail.data : undefined
  const totals = run?.view?.documents
  const questions = run ? (run.effective_questions?.length ? run.effective_questions : run.questions) : []
  const answers = new Map((run?.view?.recent || []).map(result => [result.resource, result]))
  const running = !job || !job.outcome
  const lines = job?.log || []
  // Oldest first, like a terminal: the newest line lands at the bottom, and the log follows
  // it until the reader scrolls up, and again once they scroll back down.
  const log = useRef<HTMLDivElement>(null)
  const [following, setFollowing] = useState(true)
  const latest = lines.at(-1)?.seq
  useLayoutEffect(() => {
    if (log.current && following) log.current.scrollTop = log.current.scrollHeight
  }, [following, latest, detail.dataUpdatedAt])

  return <Sheet open onOpenChange={open => !open && onClose()}>
    <SheetContent side="right" className="flex w-full flex-col gap-0 p-0 sm:max-w-[560px]">
      <div className="grid gap-3.5 border-b px-6 pt-6 pb-5">
        <span className={`inline-flex items-center gap-2 text-[11px] font-semibold uppercase tracking-[0.08em] ${running ? 'text-flame-ink' : job.outcome === 'failed' ? 'text-destructive' : 'text-muted-foreground'}`}>
          {running && <span className="size-2 rounded-full bg-flame shadow-[0_0_0_4px_var(--flame-halo)]" />}
          {running ? 'Working' : job.outcome} · {job ? seconds((job.finished_at ?? Date.now()) - job.started_at) : ''}
        </span>
        <SheetTitle className="font-serif text-[30px] leading-8 font-normal">{job ? jobTitle(job) : 'Job'}</SheetTitle>
        <SheetDescription className="font-mono text-xs">{id}{job && ` · ${job.label}`}</SheetDescription>
        {job && <Bar job={job} />}
        <div className="flex flex-wrap gap-6">
          {job?.total != null && <Count value={`${number(job.done ?? 0)} / ${number(job.total)}`} label={job.message || 'done'} />}
          {totals
            ? <>
              <Count value={number(totals.kept)} label="kept" />
              <Count value={number(totals.tagged)} label="tagged" tone="text-moss" />
              <Count value={number(totals.review)} label="review" tone="text-flame-ink" />
              <Count value={number(totals.excluded)} label="exclude" />
              <Count value={number(totals.errors)} label="errors" tone="text-destructive" />
            </>
            : <>
              <Count value={number(job?.ok ?? 0)} label="ok" tone="text-moss" />
              <Count value={number(job?.failed ?? 0)} label="failed" tone="text-destructive" />
            </>}
        </div>
        {job?.current && <div className="grid gap-0.5 text-xs"><span className="text-muted-foreground">In hand</span><span className="truncate font-mono" title={job.current}>{job.current}</span></div>}
        {job?.error && <p className="text-[13px] text-destructive">{job.error}</p>}
      </div>
      <div className="flex min-h-0 flex-1 flex-col bg-terminal py-3 font-mono text-xs leading-[22px]">
        <div className="flex justify-between px-5 pb-2 font-sans text-[11px] text-[#B9AE98]"><span>Live log · {!running ? 'finished' : following ? 'following' : 'paused, scroll down to follow'}</span><span>{job?.step}</span></div>
        <div ref={log} onScroll={event => { const { scrollHeight, scrollTop, clientHeight } = event.currentTarget; setFollowing(scrollHeight - scrollTop - clientHeight < 24) }} className="min-h-0 flex-1 overflow-y-auto">
          {lines.map(line => <LogLine key={line.seq} line={line} answer={line.type === 'item' ? answers.get(line.item.address) : undefined} questions={questions} />)}
          {!lines.length && <div className="px-5 text-[#B9AE98]">{job ? 'Waiting for the first event…' : 'This job is not running on this server.'}</div>}
        </div>
      </div>
      <div className="flex items-center gap-3 border-t px-6 py-4">
        <span className="flex-1 text-[13px] text-muted-foreground">{run ? `${run.model} · ${number(run.questions.length)} questions` : job?.kind}</span>
        {classify && <Link to="/runs" search={{ run: id, page: 1, outcome: '' }} onClick={onClose} className="inline-flex h-[34px] items-center rounded-lg px-3.5 text-[13px] font-medium shadow-[inset_0_0_0_1px_var(--input)]">Open run</Link>}
      </div>
    </SheetContent>
  </Sheet>
}

/** One line of a job's log, on the server's clock. A scored line says what Jev decided when its answer and questions are given. */
export function LogLine({ line, answer, questions = [] }: { line: JobEvent; answer?: RunResult; questions?: Parameters<typeof answerSummary>[0] }) {
  const row = (word: string, tone: string, body: ReactNode) => <div className="flex gap-3 px-5">
    <span className="w-16 shrink-0 text-[#6B6458]">{clock(line.at)}</span>
    <span className={`w-16 shrink-0 truncate ${tone}`}>{word}</span>
    <span className="flex min-w-0 flex-1 gap-3 text-[#B9AE98]">{body}</span>
  </div>
  switch (line.type) {
    case 'started': return row('started', 'text-[#B9AE98]', <span className="truncate">{line.label}</span>)
    case 'step': return row('step', 'text-terminal-ink', <span className="truncate text-terminal-ink">{line.step}</span>)
    case 'note': return row('note', 'text-[#6B6458]', <span className="truncate">{line.message}</span>)
    case 'finished': return row(line.outcome, line.outcome === 'failed' ? 'text-[#E57D6E]' : 'text-[#8FB58A]', <span className="truncate">{line.error}</span>)
    case 'item': {
      const { item } = line
      const said = answer ? answerSummary(questions, answer) : item.detail || (item.produced != null ? `${number(item.produced)} ch` : seconds(item.millis))
      return row(item.tag, verdictTone[item.verdict], <>
        {answer
          ? <DocumentLink doc={answer} className="min-w-0 truncate text-terminal-ink hover:underline">{tail(item.address)}</DocumentLink>
          : <span className={`min-w-0 truncate ${succeeded(item.verdict) ? 'text-terminal-ink' : ''}`} title={item.address}>{item.nested ? '↳ ' : ''}{tail(item.address)}</span>}
        <span className="min-w-0 flex-1 truncate">{said}</span>
      </>)
    }
    default: return null
  }
}

function Count({ value, label, tone = '' }: { value: string; label: string; tone?: string }) {
  return <span className="grid gap-0.5"><b className={`text-lg leading-[22px] font-semibold ${tone}`}>{value}</b><span className="max-w-40 truncate text-xs text-muted-foreground">{label}</span></span>
}
