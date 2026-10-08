import { useEffect, useState } from 'react'
import { keepPreviousData, useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useNavigate } from '@tanstack/react-router'
import { ArrowRight, CircleAlert, CircleCheck, ShieldCheck, X } from 'lucide-react'
import { api, type Question, type ResultOutcome, type Run, type RunDetailQuery, type RunResult } from './api'
import { Button } from '@/components/ui/button'
import { compact, money, number, plural, seconds, tail } from './format'
import { decisionOf, isChoice } from './policy'
import { DecisionBadge, DocumentLink, ErrorBox, Pulse, Spinner } from './ui'

/** A run detail as a view asks for it; polls while the run is scoring. */
export function useRunDetail(id: string, view: Partial<RunDetailQuery>, every = 1000) {
  return useQuery({
    queryKey: ['run', id, view],
    queryFn: () => api.runDetail(id, view),
    enabled: Boolean(id),
    placeholderData: keepPreviousData,
    refetchInterval: query => query.state.data?.status === 'running' ? every : false,
  })
}

/** Re-renders once a second while `on`, so waiting times count up between polls. */
function useClock(on: boolean) {
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    if (!on) return
    const timer = setInterval(() => setNow(Date.now()), 250)
    return () => clearInterval(timer)
  }, [on])
  return now
}

/** The answers that matter for one document, in a few words. */
export function answerSummary(questions: Question[], result: RunResult) {
  if (result.error) return result.error
  const parts: string[] = []
  for (const question of questions) {
    const outcome = result.outcomes?.[question.id]
    if (isChoice(question)) {
      const top = outcome?.top || result.choices?.[question.id]?.choice
      if (top) parts.push(`${top} ${(result.answers[`${question.id}:${top}`] ?? 0).toFixed(2)}`)
    } else if (outcome?.excluded || outcome?.review || outcome?.tags?.length) {
      parts.push(`${question.id} ${(result.answers[question.id] ?? 0).toFixed(2)}`)
    }
  }
  return parts.join(' · ') || 'nothing passed a threshold'
}

const segments = [
  ['excluded', 'exclude', 'Exclude'],
  ['review', 'review', 'Review'],
  ['kept', 'keep', 'Keep'],
  ['errors', 'error', 'Error'],
] as const

/**
 * A run as it happens: progress by decision, the documents out to Jev, and the answers
 * as they land. When scoring stops it becomes the run's summary, with the next step.
 */
export function LiveRun({ id, embedded, preview, onDismiss, onOutcome }: {
  id: string
  /** Inside a run's own page: no link to it, no dismiss. */
  embedded?: boolean
  preview?: boolean
  onDismiss?: () => void
  onOutcome?: (outcome: ResultOutcome) => void
}) {
  const client = useQueryClient()
  const navigate = useNavigate()
  const live = useRunDetail(id, { page: 1, page_size: 1 }, 700)
  const run = live.data
  const running = !run || run.status === 'running'
  const now = useClock(running)
  const status = run?.status
  useEffect(() => {
    if (!status || status === 'running') return
    client.invalidateQueries({ queryKey: ['runs'] })
    client.invalidateQueries({ queryKey: ['run', id] })
    client.invalidateQueries({ queryKey: ['corpus'] })
  }, [status, id, client])
  const errors = run?.view?.documents.errors || 0
  const failures = useQuery({
    queryKey: ['run-errors', id, status],
    queryFn: () => api.runDetail(id, { outcome: 'error', page_size: 200 }),
    enabled: Boolean(run && !running && errors),
  })
  const commit = useMutation({
    mutationFn: () => api.commit(id),
    onSuccess: () => Promise.all([
      client.invalidateQueries({ queryKey: ['run'] }),
      client.invalidateQueries({ queryKey: ['runs'] }),
      client.invalidateQueries({ queryKey: ['corpus'] }),
    ]),
  })

  if (live.error) return <section className="live failed"><header><CircleAlert /><b>The run could not be read</b></header><ErrorBox error={live.error} /></section>
  if (!run) return <section className="live running"><header><Spinner /><b>Starting the run…</b><span className="muted">Centinel is fixing the selection.</span></header></section>

  const view = run.view
  const totals = view?.documents
  const scored = view?.scored ?? 0
  const total = run.document_count
  const elapsed = running ? now - Date.parse(run.created_at) : run.duration_ms ?? 0
  const rate = scored && elapsed > 0 ? scored / (elapsed / 1000) : null
  const left = rate ? (total - scored) / rate * 1000 : null
  const questions = run.effective_questions?.length ? run.effective_questions : run.questions
  const toExclude = run.preview?.affected_documents || 0
  const canCommit = !preview && (run.status === 'completed' || run.status === 'committed') && toExclude > 0
  const go = (outcome: ResultOutcome) => onOutcome ? onOutcome(outcome) : !preview && navigate({ to: '/runs', search: { run: id, page: 1, outcome } })
  const failureGroups = groupErrors(failures.data?.results || [])

  return <section className={`live ${running ? 'running' : run.status === 'failed' ? 'failed' : 'done'}`} aria-live="polite">
    <header>
      {running ? <Pulse /> : run.status === 'failed' ? <CircleAlert /> : <CircleCheck />}
      <b>{running ? 'Classifying' : run.status === 'failed' ? 'The run stopped' : preview ? 'Preview finished' : 'Finished'}</b>
      <span>{number(scored)} of {plural(total, 'document')}</span>
      <span className="muted">{seconds(elapsed)}{running && rate ? ` · ${rate.toFixed(1)}/s · about ${seconds(left)} left` : ''}{!running && run.cost_usd != null ? ` · ${money(run.cost_usd)}` : ''}{!running && run.input_tokens ? ` · ${compact(run.input_tokens)} tokens` : ''}</span>
      <span className="grow" />
      {!embedded && !preview && <Button size="sm" variant="secondary" onClick={() => navigate({ to: '/runs', search: { run: id, page: 1, outcome: '' } })}>{running ? 'Watch in Runs' : 'See every result'}<ArrowRight /></Button>}
      {!embedded && onDismiss && !running && <Button size="sm" variant="ghost" aria-label="Dismiss" onClick={onDismiss}><X /></Button>}
    </header>

    <div className="progress" role="progressbar" aria-valuemin={0} aria-valuemax={total} aria-valuenow={scored}>
      {totals && segments.map(([key, tone]) => totals[key] ? <i key={key} className={tone} style={{ width: `${totals[key] / Math.max(1, total) * 100}%` }} /> : null)}
      {running && <i className="pending" style={{ width: `${(view?.in_flight?.length || 0) / Math.max(1, total) * 100}%` }} />}
    </div>

    <div className="tiles">
      {segments.map(([key, tone, label]) => <button type="button" key={key} className={`tile ${tone}`} disabled={!totals?.[key]} onClick={() => go(tone === 'keep' ? 'keep' : tone === 'error' ? 'error' : tone)}>
        <span>{label}</span><b>{number(totals?.[key] ?? 0)}</b>
      </button>)}
      <button type="button" className="tile tag" disabled={!totals?.tagged} onClick={() => go('tag')}><span>Tagged</span><b>{number(totals?.tagged ?? 0)}</b></button>
      {!!totals?.sampled && <div className="tile note"><span>Sampled</span><b>{number(totals.sampled)}</b></div>}
    </div>

    {running && <div className="live-columns">
      <div>
        <h4>Out to Jev now · {view?.in_flight?.length || 0}</h4>
        <ul className="flights">
          {(view?.in_flight || []).map(flight => <li key={`${flight.source}:${flight.resource}`}>
            <Spinner /><span className="doc" title={flight.resource}>{tail(flight.resource)}</span>
            {flight.attempt > 1 && <span className="chip warn">request {flight.attempt}</span>}
            {flight.sent_chars > 0 && <span className="muted">{compact(flight.sent_chars)} chars</span>}
            <span className="mono">{seconds(now - flight.started_ms)}</span>
          </li>)}
          {!view?.in_flight?.length && <li className="muted">{scored ? 'Waiting for the next documents…' : 'Sending the first documents…'}</li>}
        </ul>
      </div>
      <div>
        <h4>Latest answers</h4>
        <ul className="feed">
          {(view?.recent || []).map(result => <li key={`${result.source}:${result.resource}:${result.derived_sha}`} className={decisionOf(result)}>
            <DecisionBadge decision={decisionOf(result)} />
            <DocumentLink doc={result} className="doc">{tail(result.resource)}</DocumentLink>
            <span className="summary" title={answerSummary(questions, result)}>{answerSummary(questions, result)}</span>
            <span className="mono">{seconds(result.duration_ms)}</span>
          </li>)}
          {!view?.recent?.length && <li className="muted">No answers yet. The first ones usually take a few seconds.</li>}
        </ul>
      </div>
    </div>}

    {!running && <div className="next">
      {preview && <p>This was a preview. The scores are below. Nothing was saved, so it cannot be committed.</p>}
      {!preview && run.status === 'completed' && (toExclude
        ? <p>Committing removes <b>{plural(toExclude, 'document')}</b> from search and future embedding. The archive does not change, and every document can be restored.</p>
        : <p>No document reaches an exclude threshold. Nothing needs committing.</p>)}
      {run.status === 'committed' && <p>The exclusions of this run are committed.{toExclude ? ` A policy change since then would exclude ${plural(toExclude, 'more document')}.` : ''}</p>}
      {canCommit && <Button onClick={() => commit.mutate()} disabled={commit.isPending}>{commit.isPending ? <Spinner light /> : <ShieldCheck />}Commit {plural(toExclude, 'exclusion')}</Button>}
      {commit.isSuccess && <span className="success-note">Committed.</span>}
      {commit.error && <ErrorBox error={commit.error} />}
    </div>}

    {!running && errors > 0 && <div className="failures">
      <h4>{plural(errors, 'document')} failed</h4>
      {failures.isLoading && <p className="muted">Reading the errors…</p>}
      <ul>{failureGroups.map(([message, count]) => <li key={message}><b>{number(count)}×</b> {message}</li>)}</ul>
    </div>}
  </section>
}

/** Error messages with the per-document detail stripped, most frequent first. */
function groupErrors(results: RunResult[]): Array<[string, number]> {
  const counts = new Map<string, number>()
  for (const result of results) {
    const message = (result.error || '').replace(/ \(after \d+ request\(s\)\)$/, '')
    counts.set(message, (counts.get(message) || 0) + 1)
  }
  return [...counts.entries()].sort((a, b) => b[1] - a[1]).slice(0, 5)
}

export type { Run }
