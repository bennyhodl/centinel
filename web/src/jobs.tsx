import { useEffect, type ReactNode } from 'react'
import { Link } from '@tanstack/react-router'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { jobEventsUrl, type JobEvent, type JobItem, type JobState, type RunResult } from './api'
import { activeJobs, foldJob, jobTitle, succeeded } from './job-state'
import { number, seconds, tail } from './format'
import { answerSummary } from './live'
import { queries } from './queries'
import { DocumentLink } from './ui'
import { Sheet, SheetContent, SheetDescription, SheetTitle } from '@/components/ui/sheet'

/**
 * Keeps the `jobs` cache current from the server's job stream. Mounted once, in the shell:
 * the stream opens on a snapshot and then sends each change, so every page reading the
 * jobs re-renders as they move, with nothing polling. `EventSource` reconnects on its
 * own, and each reconnect opens on a fresh snapshot.
 */
export function useJobEvents() {
  const client = useQueryClient()
  useEffect(() => {
    const stream = new EventSource(jobEventsUrl)
    const key = queries.jobs().queryKey
    stream.addEventListener('snapshot', message => {
      client.setQueryData(key, (JSON.parse(message.data) as { jobs: JobState[] }).jobs)
    })
    stream.addEventListener('job', message => {
      const event = JSON.parse(message.data) as JobEvent
      client.setQueryData(key, jobs => foldJob(jobs ?? [], event))
      // A classifier run that ends has changed the ledger and possibly the corpus.
      if (event.type === 'finished' && client.getQueryData(key)?.find(job => job.id === event.job)?.kind === 'classify') {
        client.invalidateQueries({ queryKey: ['runs'] })
        client.invalidateQueries({ queryKey: ['run', event.job] })
        client.invalidateQueries({ queryKey: ['corpus'] })
      }
    })
    return () => stream.close()
  }, [client])
}

/** Every job the server knows of: running first, in the order they started, then finished. */
export function useJobs() {
  return useQuery(queries.jobs()).data ?? []
}

/** What is running right now, from any page: what each job is doing and how far along. */
export function WorkingNow({ onOpen }: { onOpen: (id: string) => void }) {
  const running = activeJobs(useJobs())
  if (!running.length) return null
  return <div className="grid gap-1 border-t pt-3 group-data-[collapsible=icon]:hidden">
    <span className="mb-1 inline-flex items-center gap-2 text-[11px] font-semibold uppercase tracking-[0.08em] text-flame-ink"><span className="size-1.5 rounded-full bg-flame shadow-[0_0_0_3px_#F6D9B4]" />Working now</span>
    {running.map(job => <button key={job.id} type="button" onClick={() => onOpen(job.id)} className="grid gap-1 rounded-md px-1.5 py-1 text-left text-[13px] hover:bg-background">
      <span className="flex justify-between gap-2"><span className="truncate">{jobTitle(job)}</span><span className="shrink-0 font-mono text-xs text-muted-foreground">{count(job)}</span></span>
      <Bar job={job} thin />
      {job.current && <span className="truncate font-mono text-[11px] text-muted-foreground">{tail(job.current)}</span>}
    </button>)}
  </div>
}

const count = (job: JobState) => job.total ? `${number(job.done ?? 0)}/${number(job.total)}` : ''

function Bar({ job, thin }: { job: JobState; thin?: boolean }) {
  const width = job.total ? (job.done ?? 0) / job.total * 100 : 0
  return <span className={`block rounded-full bg-[#EFE9DC] ${thin ? 'h-1' : 'h-1.5'}`}>
    <i className="block h-full rounded-full bg-foreground transition-[width] duration-(--motion-progress) ease-out" style={{ width: `${job.outcome ? 100 : width}%` }} />
  </span>
}

const verdictTone: Record<JobItem['verdict'], string> = { ok: 'text-[#8FB58A]', warn: 'text-[#E8A65A]', missing: 'text-[#E8A65A]', fail: 'text-[#E57D6E]' }
const clock = (ms: number) => new Date(ms).toTimeString().slice(0, 8)

/**
 * One job as it happens, line by line, on the server's clock: the stage it is in, the
 * item in hand, how far through its work list, and every page or document it finished,
 * failures in their own colour. A classifier run also reads its answers from the run, so
 * a scored line says what Jev decided.
 */
export function JobDrawer({ id, onClose }: { id: string; onClose: () => void }) {
  const job = useJobs().find(candidate => candidate.id === id)
  const classify = job?.kind === 'classify'
  const detail = useQuery({ ...queries.run(id, { page: 1, page_size: 1 }, 2000), enabled: classify })
  const run = classify ? detail.data : undefined
  const totals = run?.view?.documents
  const questions = run ? (run.effective_questions?.length ? run.effective_questions : run.questions) : []
  const answers = new Map((run?.view?.recent || []).map(result => [result.resource, result]))
  const running = !job || !job.outcome
  const lines = [...(job?.log || [])].reverse()

  return <Sheet open onOpenChange={open => !open && onClose()}>
    <SheetContent side="right" className="flex w-full flex-col gap-0 p-0 sm:max-w-[560px]">
      <div className="grid gap-3.5 border-b px-6 pt-6 pb-5">
        <span className={`inline-flex items-center gap-2 text-[11px] font-semibold uppercase tracking-[0.08em] ${running ? 'text-flame-ink' : job.outcome === 'failed' ? 'text-destructive' : 'text-muted-foreground'}`}>
          {running && <span className="size-2 rounded-full bg-flame shadow-[0_0_0_4px_#F6D9B4]" />}
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
              <Count value={number(totals.tagged)} label="tagged" tone="text-[#3E6539]" />
              <Count value={number(totals.review)} label="review" tone="text-flame-ink" />
              <Count value={number(totals.excluded)} label="exclude" />
              <Count value={number(totals.errors)} label="errors" tone="text-destructive" />
            </>
            : <>
              <Count value={number(job?.ok ?? 0)} label="ok" tone="text-[#3E6539]" />
              <Count value={number(job?.failed ?? 0)} label="failed" tone="text-destructive" />
            </>}
        </div>
        {job?.current && <div className="grid gap-0.5 text-xs"><span className="text-muted-foreground">In hand</span><span className="truncate font-mono" title={job.current}>{job.current}</span></div>}
        {job?.error && <p className="text-[13px] text-destructive">{job.error}</p>}
      </div>
      <div className="flex min-h-0 flex-1 flex-col bg-[#1F1C17] py-3 font-mono text-xs leading-[22px]">
        <div className="flex justify-between px-5 pb-2 font-sans text-[11px] text-[#B9AE98]"><span>Live log · {running ? 'following' : 'finished'}</span><span>{job?.step}</span></div>
        <div className="min-h-0 flex-1 overflow-y-auto">
          {lines.map(line => <LogLine key={line.seq} line={line} answer={line.type === 'item' ? answers.get(line.item.address) : undefined} questions={questions} />)}
          {!lines.length && <div className="px-5 text-[#B9AE98]">{job ? 'Waiting for the first event…' : 'This job is not running on this server.'}</div>}
        </div>
      </div>
      <div className="flex items-center gap-3 border-t px-6 py-4">
        <span className="flex-1 text-[13px] text-muted-foreground">{run ? `${run.model} · ${number(run.questions.length)} questions` : job?.kind}</span>
        {classify && <Link to="/runs" search={{ run: id, page: 1, outcome: '' }} onClick={onClose} className="inline-flex h-[34px] items-center rounded-lg px-3.5 text-[13px] font-medium shadow-[inset_0_0_0_1px_#CFC6B5]">Open run</Link>}
      </div>
    </SheetContent>
  </Sheet>
}

function LogLine({ line, answer, questions }: { line: JobEvent; answer?: RunResult; questions: Parameters<typeof answerSummary>[0] }) {
  const row = (word: string, tone: string, body: ReactNode) => <div className="flex gap-3 px-5">
    <span className="w-16 shrink-0 text-[#6B6458]">{clock(line.at)}</span>
    <span className={`w-16 shrink-0 truncate ${tone}`}>{word}</span>
    <span className="flex min-w-0 flex-1 gap-3 text-[#B9AE98]">{body}</span>
  </div>
  switch (line.type) {
    case 'started': return row('started', 'text-[#B9AE98]', <span className="truncate">{line.label}</span>)
    case 'step': return row('step', 'text-[#F4EEE1]', <span className="truncate text-[#F4EEE1]">{line.step}</span>)
    case 'note': return row('note', 'text-[#6B6458]', <span className="truncate">{line.message}</span>)
    case 'finished': return row(line.outcome, line.outcome === 'failed' ? 'text-[#E57D6E]' : 'text-[#8FB58A]', <span className="truncate">{line.error}</span>)
    case 'item': {
      const { item } = line
      const said = answer ? answerSummary(questions, answer) : item.detail || (item.produced != null ? `${number(item.produced)} ch` : seconds(item.millis))
      return row(item.tag, verdictTone[item.verdict], <>
        {answer
          ? <DocumentLink doc={answer} className="min-w-0 truncate text-[#F4EEE1] hover:underline">{tail(item.address)}</DocumentLink>
          : <span className={`min-w-0 truncate ${succeeded(item.verdict) ? 'text-[#F4EEE1]' : ''}`} title={item.address}>{item.nested ? '↳ ' : ''}{tail(item.address)}</span>}
        <span className="min-w-0 flex-1 truncate">{said}</span>
      </>)
    }
    default: return null
  }
}

function Count({ value, label, tone = '' }: { value: string; label: string; tone?: string }) {
  return <span className="grid gap-0.5"><b className={`text-lg leading-[22px] font-semibold ${tone}`}>{value}</b><span className="max-w-40 truncate text-xs text-muted-foreground">{label}</span></span>
}
