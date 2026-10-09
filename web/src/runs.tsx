import React, { useEffect, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useNavigate, useSearch } from '@tanstack/react-router'
import { ChevronLeft, ChevronRight, RotateCcw } from 'lucide-react'
import { api, type ResultOutcome, type Run, type RunDetailQuery } from './api'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { money, number, plural, seconds, tail } from './format'
import { LiveRun, useRunDetail } from './live'
import { ResultsSection } from './results'
import { DocumentLink, Empty, ErrorBox, PageHeader, Pulse, Spinner } from './ui'

const startView = (outcome: string): RunDetailQuery => ({ page: 1, page_size: 100, outcome: outcome as ResultOutcome, sort: 'decision', direction: '' })

export function Runs() {
  const search = useSearch({ from: '/runs' })
  const navigate = useNavigate({ from: '/runs' })
  const query = useQuery({
    queryKey: ['runs', search.page],
    queryFn: () => api.runs(search.page, 25),
    refetchInterval: current => current.state.data?.runs.some(run => run.status === 'running') ? 2000 : false,
  })
  const selectedId = search.run || query.data?.runs[0]?.id || ''
  const [view, setView] = useState<RunDetailQuery>(() => startView(search.outcome))
  useEffect(() => setView(startView(search.outcome)), [selectedId, search.outcome])
  const detail = useRunDetail(selectedId, view)
  const pages = Math.max(1, Math.ceil((query.data?.total || 0) / (query.data?.page_size || 25)))
  return <><PageHeader eyebrow="Run ledger" title="Runs" detail="Every run keeps its questions, its documents, and every answer. Check the decisions, then commit the exclusions." />
    {query.error ? <ErrorBox error={query.error} /> : <div className="grid items-start gap-4 xl:grid-cols-[220px_minmax(0,1fr)]"><section className="rounded-lg border bg-card text-card-foreground shadow-sm p-1 [&_button]:w-full [&_button]:text-left [&_button]:rounded-md [&_button]:p-3 [&_button]:flex [&_button]:items-center [&_button]:justify-between [&_button]:gap-2 [&_button]:hover:bg-accent [&_small]:block [&_small]:text-xs [&_small]:text-muted-foreground">
      <div className="flex min-h-11 items-center justify-between border-b px-4 text-sm font-medium [&_small]:text-muted-foreground"><span>Runs</span><small>{number(query.data?.total)}</small></div>
      {query.data?.runs.map(item => <button key={item.id} className={item.id === selectedId ? 'bg-accent' : ''} onClick={() => navigate({ search: previous => ({ ...previous, run: item.id, outcome: '' }) })}>
        <span><b>{item.created_at.slice(0, 16).replace('T', ' ')}</b><small>{plural(item.document_count, 'doc')} · {item.errors ? `${number(item.errors)} errors · ` : ''}{item.model}</small></span>
        {item.status === 'running' ? <span className="inline-flex items-center gap-1 text-xs"><Pulse />running</span> : <Badge variant={item.status === 'failed' || item.status === 'interrupted' ? 'outline' : item.status === 'committed' ? 'default' : 'secondary'}>{item.status}</Badge>}
      </button>)}
      {query.data && !query.data.runs.length && <Empty>No runs yet. Start one on the Classify page.</Empty>}
      {query.data && pages > 1 && <div className="flex items-center justify-center gap-2 border-t p-2 text-xs"><Button aria-label="Previous run page" variant="ghost" size="icon" disabled={search.page === 1} onClick={() => navigate({ search: { run: '', page: search.page - 1, outcome: '' } })}><ChevronLeft /></Button><span>{search.page} / {pages}</span><Button aria-label="Next run page" variant="ghost" size="icon" disabled={search.page >= pages} onClick={() => navigate({ search: { run: '', page: search.page + 1, outcome: '' } })}><ChevronRight /></Button></div>}
    </section>
      {detail.error ? <ErrorBox error={detail.error} /> : detail.data ? <RunDetail run={detail.data} view={view} setView={setView} loading={detail.isFetching} /> : selectedId ? <div className="flex items-center gap-2 py-6 text-sm text-muted-foreground"><Spinner /> Reading the run…</div> : null}
    </div>}
  </>
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
