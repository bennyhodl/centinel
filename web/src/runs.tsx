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
    {query.error ? <ErrorBox error={query.error} /> : <div className="runs-grid"><section className="panel run-list">
      <div className="panel-top"><span>Runs</span><small>{number(query.data?.total)}</small></div>
      {query.data?.runs.map(item => <button key={item.id} className={item.id === selectedId ? 'active' : ''} onClick={() => navigate({ search: previous => ({ ...previous, run: item.id, outcome: '' }) })}>
        <span><b>{item.created_at.slice(0, 16).replace('T', ' ')}</b><small>{plural(item.document_count, 'doc')} · {item.errors ? `${number(item.errors)} errors · ` : ''}{item.model}</small></span>
        {item.status === 'running' ? <span className="running-badge"><Pulse />running</span> : <Badge variant={item.status === 'failed' || item.status === 'interrupted' ? 'warning' : item.status === 'committed' ? 'success' : 'muted'}>{item.status}</Badge>}
      </button>)}
      {query.data && !query.data.runs.length && <Empty>No runs yet. Start one on the Classify page.</Empty>}
      {query.data && pages > 1 && <div className="run-pager"><Button aria-label="Previous run page" variant="ghost" size="icon" disabled={search.page === 1} onClick={() => navigate({ search: { run: '', page: search.page - 1, outcome: '' } })}><ChevronLeft /></Button><span>{search.page} / {pages}</span><Button aria-label="Next run page" variant="ghost" size="icon" disabled={search.page >= pages} onClick={() => navigate({ search: { run: '', page: search.page + 1, outcome: '' } })}><ChevronRight /></Button></div>}
    </section>
      {detail.error ? <ErrorBox error={detail.error} /> : detail.data ? <RunDetail run={detail.data} view={view} setView={setView} loading={detail.isFetching} /> : selectedId ? <div className="loading"><Spinner /> Reading the run…</div> : null}
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
  return <section className="run-detail">
    <div className="run-title">
      <div><span className="eyebrow">{run.model} · evaluated as of {run.evaluation_date}</span><h2>{plural(run.document_count, 'document')} × {plural(run.questions.length, 'question')}</h2><small className="run-id">{run.id}</small></div>
      <div className="run-actions"><Button variant="secondary" disabled={running || repeat.isPending || !inputTotal} onClick={() => repeat.mutate()}><RotateCcw />{repeat.isPending ? 'Starting…' : 'Run again'}</Button></div>
    </div>
    {repeat.error && <ErrorBox error={repeat.error} />}
    <LiveRun id={run.id} embedded onOutcome={outcome => setView({ ...view, outcome, page: 1 })} />
    <Tabs defaultValue="results"><TabsList><TabsTrigger value="results">Results</TabsTrigger><TabsTrigger value="questions">Questions</TabsTrigger><TabsTrigger value="inputs">Inputs · {number(inputTotal)}</TabsTrigger><TabsTrigger value="settings">Settings</TabsTrigger></TabsList>
      <TabsContent value="results"><ResultsSection run={run} questions={questions} view={view} setView={setView} loading={loading} /></TabsContent>
      <TabsContent value="questions"><div className="question-ledger">{questions.map(question => <div key={question.id}><h4>{question.id} <small>v{question.version} · {question.kind === 'choice' ? 'choice' : 'yes / no'}</small></h4><p>{question.instructions}</p>{question.options?.length ? <ul>{question.options.map(option => <li key={option.id}><b>{option.id}</b> <em>{option.action}</em> {option.description}</li>)}</ul> : null}</div>)}</div></TabsContent>
      <TabsContent value="inputs"><div className="input-ledger">{(run.inputs || []).map((doc, index) => <DocumentLink doc={doc} key={`${doc.source}:${doc.resource}:${doc.derived_sha}`}><span>{String(index + 1).padStart(2, '0')}</span><div><b>{tail(doc.resource)}</b><small>{doc.source} · {doc.derived_sha}</small></div></DocumentLink>)}</div>{inputTotal > run.inputs.length && <p className="more-note">The first {number(run.inputs.length)} of {number(inputTotal)} inputs. The run keeps all of them, and “Run again” uses all of them.</p>}</TabsContent>
      <TabsContent value="settings"><dl className="settings-list"><dt>Model</dt><dd>{run.model}</dd><dt>Input tokens</dt><dd>{number(run.input_tokens)}</dd><dt>Cost</dt><dd>{money(run.cost_usd)}</dd><dt>Duration</dt><dd>{seconds(run.duration_ms)}</dd><dt>Throughput</dt><dd>{run.throughput_docs_sec == null ? 'Unknown' : `${run.throughput_docs_sec.toFixed(2)} documents a second`}</dd>{Object.entries(run.settings || {}).map(([key, value]) => <React.Fragment key={key}><dt>{key.replaceAll('_', ' ')}</dt><dd className="mono">{typeof value === 'string' ? value || 'Any' : JSON.stringify(value)}</dd></React.Fragment>)}</dl></TabsContent>
    </Tabs>
  </section>
}
