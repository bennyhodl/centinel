import React, { useEffect, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useNavigate, useSearch } from '@tanstack/react-router'
import { ChevronLeft, ChevronRight, RotateCcw } from 'lucide-react'
import { api, type ResultOutcome, type Run, type RunDetailQuery, type RunSummary } from './api'
import { Button } from '@/components/ui/button'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { money, number, plural, seconds, tail } from './format'
import { LiveRun, useRunDetail } from './live'
import { ResultsSection } from './results'
import { DocumentLink, Empty, ErrorBox, PageHeader, Spinner } from './ui'

const startView = (outcome: string): RunDetailQuery => ({ page: 1, page_size: 100, outcome: outcome as ResultOutcome, sort: 'decision', direction: '' })

export function Runs() {
  const search = useSearch({ from: '/runs' })
  const navigate = useNavigate({ from: '/runs' })
  const query = useQuery({
    queryKey: ['runs', search.page],
    queryFn: () => api.runs(search.page, 25),
    refetchInterval: current => current.state.data?.runs.some(run => run.status === 'running') ? 2000 : false,
  })
  const [view, setView] = useState<RunDetailQuery>(() => startView(search.outcome))
  useEffect(() => setView(startView(search.outcome)), [search.run, search.outcome])
  const detail = useRunDetail(search.run, view)
  const pages = Math.max(1, Math.ceil((query.data?.total || 0) / (query.data?.page_size || 25)))
  const open = (run: string) => navigate({ search: { run, page: search.page, outcome: '' } })

  if (search.run) return <>
    <PageHeader eyebrow={<button type="button" onClick={() => open('')} className="inline-flex items-center gap-1 hover:text-foreground"><ChevronLeft className="size-3.5" />All runs</button>} title="Run" />
    {detail.error ? <ErrorBox error={detail.error} /> : detail.data ? <RunDetail run={detail.data} view={view} setView={setView} loading={detail.isFetching} /> : <div className="flex items-center gap-2 py-6 text-sm text-muted-foreground"><Spinner /> Reading the run…</div>}
  </>

  const running = query.data?.runs.filter(run => run.status === 'running') || []
  return <>
    <PageHeader title="Runs" detail="Each run asks Jev your questions about a set of documents. Nothing changes in the corpus until you commit it." />
    {query.error && <ErrorBox error={query.error} />}
    {running.map(run => <LiveRun key={run.id} id={run.id} />)}
    <section className="mt-4">
      <div className="flex h-8 items-center gap-6 border-b border-foreground text-[11px] font-semibold uppercase tracking-[0.08em] text-muted-foreground">
        <span className="w-36 shrink-0">Started</span><span className="flex-1">Asked</span><span className="hidden w-52 shrink-0 md:block">Outcome</span><span className="w-44 shrink-0 text-right">Next</span>
      </div>
      {query.data?.runs.map(run => <div key={run.id} role="button" tabIndex={0} onClick={() => open(run.id)} onKeyDown={event => event.key === 'Enter' && open(run.id)} className="flex cursor-pointer items-center gap-6 border-b py-4 hover:bg-[#FBF9F4]">
        <span className="grid w-36 shrink-0 gap-0.5"><b className="text-sm font-semibold">{run.created_at.slice(0, 16).replace('T', ' ')}</b><span className="text-xs text-muted-foreground">{seconds(run.duration_ms)}{run.cost_usd != null ? ` · ${money(run.cost_usd)}` : ''}</span></span>
        <span className="grid min-w-0 flex-1 gap-0.5"><span className="truncate text-sm">{plural(run.document_count, 'document')}</span><span className="truncate font-mono text-xs text-muted-foreground">{run.id} · {run.model}</span></span>
        <span className={`hidden w-52 shrink-0 text-[13px] md:block ${run.status === 'failed' || run.status === 'interrupted' ? 'text-destructive' : run.status === 'committed' ? 'text-muted-foreground' : ''}`}>{outcomeLine(run)}</span>
        <span className="flex w-44 shrink-0 justify-end"><NextStep run={run} /></span>
      </div>)}
      {query.data && !query.data.runs.length && <Empty>No runs yet. Start one on the Classify page.</Empty>}
      {query.data && pages > 1 && <div className="flex items-center justify-end gap-2 py-4 text-[13px]"><Button variant="outline" size="sm" disabled={search.page === 1} onClick={() => navigate({ search: { run: '', page: search.page - 1, outcome: '' } })}><ChevronLeft />Previous</Button><span>{search.page} / {pages}</span><Button variant="outline" size="sm" disabled={search.page >= pages} onClick={() => navigate({ search: { run: '', page: search.page + 1, outcome: '' } })}>Next<ChevronRight /></Button></div>}
    </section>
  </>
}

/** What a run came to, in words. */
function outcomeLine(run: RunSummary) {
  if (run.status === 'running') return 'Scoring now'
  if (run.status === 'committed') return 'Committed'
  if (run.status === 'failed' || run.status === 'interrupted') return `Stopped${run.errors ? ` · ${plural(run.errors, 'error')}` : ''}`
  return run.errors ? `Finished · ${plural(run.errors, 'error')}` : 'Finished'
}

/** The one thing to do with a run next. Commit and review happen on the run itself. */
function NextStep({ run }: { run: RunSummary }) {
  if (run.status === 'running') return <span className="text-xs font-semibold text-flame-ink">Watch →</span>
  if (run.status === 'completed') return <Button size="sm">Open results</Button>
  if (run.status === 'failed' || run.status === 'interrupted') return <Button size="sm" variant="outline">See what failed</Button>
  return <Button size="sm" variant="outline">Open</Button>
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
