import React, { useEffect, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useNavigate, useSearch } from '@tanstack/react-router'
import { ChevronLeft, ChevronRight, RotateCcw } from 'lucide-react'
import { api, type ResultOutcome, type Run, type RunDetailQuery, type RunSummary } from './api'
import { queries } from './queries'
import { Button } from '@/components/ui/button'
import { Skeleton } from '@/components/ui/skeleton'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { money, number, plural, seconds, tail } from './format'
import { LiveRun, useRunDetail } from './live'
import { ResultsSection } from './results'
import { DocumentLink, Empty, ErrorBox, PageHeader, Spinner } from './ui'

export const startView = (outcome: string): RunDetailQuery => ({ page: 1, page_size: 100, outcome: outcome as ResultOutcome, sort: 'decision', direction: '' })

export function Runs() {
  const search = useSearch({ from: '/runs' })
  const navigate = useNavigate({ from: '/runs' })
  const query = useQuery(queries.runs(search.page))
  const [view, setView] = useState<RunDetailQuery>(() => startView(search.outcome))
  useEffect(() => setView(startView(search.outcome)), [search.run, search.outcome])
  const detail = useRunDetail(search.run, view)
  const pages = Math.max(1, Math.ceil((query.data?.total || 0) / (query.data?.page_size || 25)))
  const open = (run: string) => navigate({ search: { run, page: search.page, outcome: '' } })

  if (search.run) return <>
    <PageHeader eyebrow={<button type="button" onClick={() => open('')} className="inline-flex items-center gap-1 hover:text-foreground"><ChevronLeft className="size-3.5" />All runs</button>} title="Run" />
    {detail.error ? <ErrorBox error={detail.error} /> : detail.data ? <RunDetail run={detail.data} view={view} setView={setView} loading={detail.isFetching} /> : <div className="flex items-center gap-2 py-6 text-sm text-muted-foreground"><Spinner /> Reading the run…</div>}
  </>

  const runs = query.data?.runs || []
  const running = runs.filter(run => run.status === 'running')
  const weekAgo = Date.now() - 7 * 86_400_000
  const thisWeek = runs.filter(run => Date.parse(run.created_at) >= weekAgo)
  const spent = thisWeek.reduce((sum, run) => sum + (run.cost_usd || 0), 0)
  return <>
    <PageHeader title="Runs" detail="Each run asks Jev your questions about a set of documents. Nothing changes in the corpus until you commit it.">
      <div className="flex shrink-0 items-end gap-9">
        <Stat value={spent ? money(spent) : '$0'} label={`this week · ${plural(thisWeek.length, 'run')}`} />
        <Stat value={running.length ? `${running.length} live` : 'None live'} label={running.length ? 'scoring now' : 'nothing scoring'} flame={running.length > 0} />
      </div>
    </PageHeader>
    {query.error && <ErrorBox error={query.error} />}
    <div className="grid gap-4">{running.map(run => <RunHero key={run.id} id={run.id} onOpen={() => open(run.id)} />)}</div>
    <section className="mt-6">
      <div className="flex h-8 items-center gap-6 border-b border-foreground text-[11px] font-semibold uppercase tracking-[0.08em] text-muted-foreground">
        <span className="w-36 shrink-0">Started</span><span className="flex-1">Asked</span><span className="hidden w-56 shrink-0 md:block">Outcome</span><span className="w-52 shrink-0 text-right">Next</span>
      </div>
      {runs.filter(run => run.status !== 'running').map(run => <HistoryRow key={run.id} summary={run} onOpen={outcome => navigate({ search: { run: run.id, page: search.page, outcome } })} />)}
      {query.data && !runs.length && <Empty>No runs yet. Start one on the Classify page.</Empty>}
      {query.data && pages > 1 && <div className="flex items-center justify-end gap-2 py-4 text-[13px]"><Button variant="outline" size="sm" disabled={search.page === 1} onClick={() => navigate({ search: { run: '', page: search.page - 1, outcome: '' } })}><ChevronLeft />Previous</Button><span>{search.page} / {pages}</span><Button variant="outline" size="sm" disabled={search.page >= pages} onClick={() => navigate({ search: { run: '', page: search.page + 1, outcome: '' } })}>Next<ChevronRight /></Button></div>}
    </section>
  </>
}

function Stat({ value, label, flame }: { value: string; label: string; flame?: boolean }) {
  return <span className="grid justify-items-end gap-0.5"><b className={`text-2xl leading-7 font-semibold tracking-[-0.02em] ${flame ? 'text-flame-ink' : ''}`}>{value}</b><span className="text-xs text-muted-foreground">{label}</span></span>
}

/** What a run asks, in words: the question itself when there is one, the ids when there are several. */
const asked = (run: Run) => run.questions.length === 1 ? run.questions[0].instructions || run.questions[0].id : run.questions.map(question => question.id).join(' · ')

/** One colour per decision, as on the live panel. */
const segments = [
  ['kept', 'bg-foreground', 'keep'],
  ['review', 'bg-flame', 'need you'],
  ['excluded', 'bg-[#B9AE98]', 'exclude'],
  ['errors', 'bg-destructive', 'errors'],
] as const

/** A run that is scoring now: what it asks, how far it is, and how its documents are falling. */
function RunHero({ id, onOpen }: { id: string; onOpen: () => void }) {
  const detail = useRunDetail(id, { page: 1, page_size: 1 }, 700)
  const run = detail.data
  const [now, setNow] = useState(Date.now())
  useEffect(() => { const timer = setInterval(() => setNow(Date.now()), 1000); return () => clearInterval(timer) }, [])
  if (!run) return <div className="flex items-center gap-2 rounded-xl border border-[#F0D6B0] bg-[#FFFBF4] p-6 text-sm text-flame-ink"><Spinner />Reading the run…</div>
  const totals = run.view?.documents
  const scored = run.view?.scored ?? 0
  const elapsed = now - Date.parse(run.created_at)
  const rate = scored && elapsed > 0 ? scored / elapsed : 0
  const left = rate ? (run.document_count - scored) / rate : null
  return <section className="grid gap-[18px] rounded-xl border border-[#F0D6B0] bg-[#FFFBF4] p-6">
    <div className="flex items-start justify-between gap-6">
      <div className="grid gap-1.5">
        <span className="inline-flex items-center gap-2 text-xs font-semibold uppercase tracking-[0.08em] text-flame-ink"><span className="size-2 rounded-full bg-flame shadow-[0_0_0_4px_#F6D9B4]" />Scoring now · {seconds(elapsed)}</span>
        <h2 className="text-[22px] leading-7 font-semibold tracking-[-0.01em]">{asked(run)}</h2>
        <span className="text-sm text-muted-foreground">{plural(run.document_count, 'document')} · {plural(run.questions.length, 'question')} · {run.model}</span>
      </div>
      <Button variant="outline" className="shrink-0 bg-background" onClick={onOpen}>Open</Button>
    </div>
    <div className="grid gap-3">
      <span className="flex h-2.5 overflow-hidden rounded-full bg-[#EFE9DC]">{totals && segments.map(([key, colour]) => totals[key] ? <i key={key} className={colour} style={{ width: `${totals[key] / Math.max(1, run.document_count) * 100}%` }} /> : null)}</span>
      <div className="flex flex-wrap items-center gap-x-7 gap-y-2">
        {segments.map(([key, colour, label]) => <span key={key} className="inline-flex items-center gap-2 text-[13px]"><i className={`size-2.5 rounded-[2px] ${colour}`} /><b className="font-semibold">{number(totals?.[key] ?? 0)}</b>{label}</span>)}
        <span className="ml-auto text-[13px] text-muted-foreground">{number(scored)} of {number(run.document_count)}{left != null ? ` · ~${seconds(left)} left` : ''} · {number(run.view?.in_flight?.length ?? 0)} in flight to Jev</span>
      </div>
    </div>
  </section>
}

/** A finished run in the ledger, with the one thing to do with it next. */
function HistoryRow({ summary, onOpen }: { summary: RunSummary; onOpen: (outcome: ResultOutcome) => void }) {
  const detail = useRunDetail(summary.id, { page: 1, page_size: 1 })
  const run = detail.data
  const totals = run?.view?.documents
  const failed = summary.status === 'failed' || summary.status === 'interrupted'
  const outcome = !totals ? '' : summary.status === 'committed'
    ? `Committed · ${number(totals.excluded)} excluded`
    : failed ? `Stopped · ${plural(totals.errors, 'error')}`
      : [totals.kept && `${number(totals.kept)} keep`, totals.tagged && `${number(totals.tagged)} tagged`, totals.excluded && `${number(totals.excluded)} exclude`, totals.errors && `${number(totals.errors)} errors`].filter(Boolean).join(' · ') || 'Nothing decided'
  return <div role="button" tabIndex={0} onClick={() => onOpen('')} onKeyDown={event => event.key === 'Enter' && onOpen('')} className="flex cursor-pointer items-center gap-6 border-b py-4 hover:bg-[#FBF9F4]">
    <span className="grid w-36 shrink-0 gap-0.5"><b className="text-sm font-semibold">{summary.created_at.slice(0, 16).replace('T', ' ')}</b><span className="text-xs text-muted-foreground">{seconds(summary.duration_ms)}{summary.cost_usd != null ? ` · ${money(summary.cost_usd)}` : ''}</span></span>
    <span className="grid min-w-0 flex-1 gap-0.5"><span className="truncate text-sm">{run ? `${asked(run)} · ${plural(summary.document_count, 'document')}` : plural(summary.document_count, 'document')}</span><span className="truncate font-mono text-xs text-muted-foreground">{summary.id} · {summary.model}</span></span>
    <span className={`hidden w-56 shrink-0 text-[13px] md:block ${failed ? 'text-destructive' : summary.status === 'committed' ? 'text-muted-foreground' : ''}`}>{outcome}</span>
    <span className="flex w-52 shrink-0 justify-end" onClick={event => event.stopPropagation()}>{run && <NextStep run={run} onOpen={onOpen} />}</span>
  </div>
}

/** The one thing to do with a run next: commit its exclusions, review what it was unsure of, or run it again. */
function NextStep({ run, onOpen }: { run: Run; onOpen: (outcome: ResultOutcome) => void }) {
  const client = useQueryClient()
  const [armed, setArmed] = useState(false)
  const commit = useMutation({
    mutationFn: () => api.commit(run.id),
    onSuccess: () => Promise.all([client.invalidateQueries({ queryKey: ['run'] }), client.invalidateQueries({ queryKey: ['runs'] }), client.invalidateQueries({ queryKey: ['corpus'] })]),
  })
  const repeat = useMutation({
    mutationFn: () => api.run({ repeat: run.id, questions: run.questions, model: run.model, evaluation_date: run.evaluation_date, settings: { concurrency: run.settings.concurrency ?? 8 } }),
    onSuccess: () => client.invalidateQueries({ queryKey: ['runs'] }),
  })
  const toExclude = run.preview?.affected_documents || 0
  const review = run.view?.documents.review || 0
  if (run.status === 'completed' && toExclude) return <Button size="sm" disabled={commit.isPending} onClick={() => armed ? commit.mutate() : setArmed(true)} onBlur={() => setArmed(false)}>{commit.isPending ? <Spinner /> : null}{armed ? 'Click again to commit' : `Commit ${plural(toExclude, 'exclusion')}`}</Button>
  if (review) return <button type="button" onClick={() => onOpen('review')} className="h-8 rounded-md bg-flame-soft px-3 text-[13px] font-semibold text-flame-ink shadow-[inset_0_0_0_1px_#F0D6B0]">Review {number(review)} →</button>
  if (run.status === 'failed' || run.status === 'interrupted') return <Button size="sm" variant="outline" disabled={repeat.isPending} onClick={() => repeat.mutate()}>{repeat.isPending ? <Spinner /> : <RotateCcw />}Run again</Button>
  return <Button size="sm" variant="outline" onClick={() => onOpen('')}>Open</Button>
}

function RunDetail({ run, view, setView, loading }: { run: Run; view: RunDetailQuery; setView: (view: RunDetailQuery) => void; loading: boolean }) {
  const client = useQueryClient()
  const navigate = useNavigate({ from: '/runs' })
  const running = run.status === 'running'
  const repeat = useMutation({
    mutationFn: () => api.run({ repeat: run.id, questions: run.questions, model: run.model, evaluation_date: run.evaluation_date, settings: { concurrency: run.settings.concurrency ?? 8 } }),
    onSuccess: response => { client.invalidateQueries({ queryKey: ['runs'] }); navigate({ search: { run: response.id, page: 1, outcome: '' } }) },
  })
  const questions = run.effective_questions?.length ? run.effective_questions : run.questions
  const inputTotal = run.view?.input_total ?? run.inputs.length
  return <section className="min-w-0 rounded-lg border bg-card p-5">
    <div className="flex flex-wrap items-start justify-between gap-5 [&_h2]:mt-1 [&_h2]:text-lg [&_h2]:font-semibold">
      <div><span className="text-xs font-medium text-muted-foreground">{run.model} · evaluated as of {run.evaluation_date}</span><h2>{plural(run.document_count, 'document')} × {plural(run.questions.length, 'question')}</h2><small className="mt-1 block font-mono text-xs text-muted-foreground">{run.id}</small></div>
      <div className="flex gap-2"><Button variant="secondary" disabled={running || repeat.isPending || !inputTotal} onClick={() => repeat.mutate()}><RotateCcw />{repeat.isPending ? 'Starting…' : 'Run again'}</Button></div>
    </div>
    {repeat.error && <ErrorBox error={repeat.error} />}
    <LiveRun id={run.id} embedded onOutcome={outcome => setView({ ...view, outcome, page: 1 })} />
    <Tabs defaultValue="results"><TabsList><TabsTrigger value="results">Results</TabsTrigger><TabsTrigger value="questions">Questions</TabsTrigger><TabsTrigger value="inputs">Inputs · {number(inputTotal)}</TabsTrigger><TabsTrigger value="settings">Settings</TabsTrigger></TabsList>
      <TabsContent value="results"><ResultsSection run={run} questions={questions} view={view} setView={setView} loading={loading} /></TabsContent>
      <TabsContent value="questions"><div className="grid gap-3 [&>div]:rounded-md [&>div]:border [&>div]:p-3 [&_h4]:mb-2 [&_h4]:font-mono [&_h4]:text-sm [&_small]:text-muted-foreground [&_p]:text-sm [&_ul]:mt-2 [&_ul]:list-disc [&_ul]:pl-4 [&_li]:text-sm [&_em]:text-xs [&_em]:not-italic [&_em]:text-muted-foreground">{questions.map(question => <div key={question.id}><h4>{question.id} <small>v{question.version} · {question.kind === 'choice' ? 'choice' : 'yes / no'}</small></h4><p>{question.instructions}</p>{question.options?.length ? <ul>{question.options.map(option => <li key={option.id}><b>{option.id}</b> <em>{option.action}</em> {option.description}</li>)}</ul> : null}</div>)}</div></TabsContent>
      <TabsContent value="inputs"><div className="grid [&_a]:grid [&_a]:grid-cols-[34px_minmax(0,1fr)] [&_a]:gap-2 [&_a]:border-t [&_a]:p-2 [&_small]:block [&_small]:truncate [&_small]:text-xs [&_small]:text-muted-foreground [&_b]:block [&_b]:truncate [&_b]:text-sm">{(run.inputs || []).map((doc, index) => <DocumentLink doc={doc} key={`${doc.source}:${doc.resource}:${doc.derived_sha}`}><span>{String(index + 1).padStart(2, '0')}</span><div><b>{tail(doc.resource)}</b><small>{doc.source} · {doc.derived_sha}</small></div></DocumentLink>)}</div>{inputTotal > run.inputs.length && <p className="m-2 text-xs text-muted-foreground">The first {number(run.inputs.length)} of {number(inputTotal)} inputs. The run keeps all of them, and “Run again” uses all of them.</p>}</TabsContent>
      <TabsContent value="settings"><dl className="grid grid-cols-[160px_minmax(0,1fr)] text-sm [&_dt]:border-t [&_dt]:p-2 [&_dt]:text-muted-foreground [&_dd]:border-t [&_dd]:p-2 [&_dd]:wrap-anywhere"><dt>Model</dt><dd>{run.model}</dd><dt>Input tokens</dt><dd>{number(run.input_tokens)}</dd><dt>Cost</dt><dd>{money(run.cost_usd)}</dd><dt>Duration</dt><dd>{seconds(run.duration_ms)}</dd><dt>Throughput</dt><dd>{run.throughput_docs_sec == null ? 'Unknown' : `${run.throughput_docs_sec.toFixed(2)} documents a second`}</dd>{Object.entries(run.settings || {}).map(([key, value]) => <React.Fragment key={key}><dt>{key.replaceAll('_', ' ')}</dt><dd className="font-mono">{typeof value === 'string' ? value || 'Any' : JSON.stringify(value)}</dd></React.Fragment>)}</dl></TabsContent>
    </Tabs>
  </section>
}

/** The ledger before it arrives: the real header and columns, each row masked. */
export function RunsSkeleton() {
  return <div aria-busy>
    <PageHeader title="Runs" detail="Each run asks Jev your questions about a set of documents. Nothing changes in the corpus until you commit it." />
    <section className="mt-6">
      <div className="flex h-8 items-center gap-6 border-b border-foreground text-[11px] font-semibold uppercase tracking-[0.08em] text-muted-foreground">
        <span className="w-36 shrink-0">Started</span><span className="flex-1">Asked</span><span className="hidden w-56 shrink-0 md:block">Outcome</span><span className="w-52 shrink-0 text-right">Next</span>
      </div>
      {Array.from({ length: 5 }, (_, i) => <div key={i} className="flex items-center gap-6 border-b py-4">
        <span className="grid w-36 shrink-0 gap-0.5"><b className="text-sm font-semibold"><Skeleton mask="2026-09-17 16:41" /></b><span className="text-xs"><Skeleton mask="21.0 s · $0.03" /></span></span>
        <span className="grid min-w-0 flex-1 gap-0.5"><span className="text-sm"><Skeleton mask="poor_extraction · navigation_shell · 200 documents" /></span><span className="font-mono text-xs"><Skeleton mask="run-1789663310435115000 · jev-1.13.0" /></span></span>
        <span className="hidden w-56 shrink-0 text-[13px] md:block"><Skeleton mask="148 keep · 60 tagged" /></span>
        <span className="flex w-52 shrink-0 justify-end"><Skeleton className="h-8 w-16" /></span>
      </div>)}
    </section>
  </div>
}
