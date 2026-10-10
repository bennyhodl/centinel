import { useEffect, useState, type ReactNode } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useNavigate } from '@tanstack/react-router'
import { ArrowRight, CircleAlert, CircleCheck, ShieldCheck, X } from 'lucide-react'
import { api, type Question, type ResultOutcome, type Run, type RunDetailQuery, type RunResult } from './api'
import { queries } from './queries'
import { Button } from '@/components/ui/button'
import { compact, money, number, plural, seconds, tail } from './format'
import { decisionOf, isChoice } from './policy'
import { DecisionBadge, DocumentLink, ErrorBox, Pulse, Spinner } from './ui'

/** A run detail as a view asks for it; polls while the run is scoring. */
export function useRunDetail(id: string, view: Partial<RunDetailQuery>, every = 1000) {
  return useQuery(queries.run(id, view, every))
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

/**
 * One colour per decision: ink is kept, the flame needs a person, faded ink is excluded.
 * "Not asked" only shows when a run has some: documents no question of the chain reached.
 */
const segments = [
  { key: 'kept', outcome: 'keep', label: 'keep', colour: 'bg-foreground' },
  { key: 'review', outcome: 'review', label: 'need you', colour: 'bg-flame' },
  { key: 'excluded', outcome: 'exclude', label: 'exclude', colour: 'bg-stone' },
  { key: 'not_asked', outcome: 'not_asked', label: 'not asked', colour: 'bg-rule', quiet: true },
  { key: 'errors', outcome: 'error', label: 'errors', colour: 'bg-destructive' },
] as const

/** What a run asks, in words: the question itself when there is one, the ids when there are several. */
export const asked = (run: Run) => run.questions.length === 1 ? run.questions[0].instructions || run.questions[0].id : run.questions.map(question => question.id).join(' · ')

/**
 * A run in one card: what it asks, how far it is, and how its documents fell. While it
 * scores, the documents out to Jev and the answers as they land; once it stops, the
 * next step. The Runs ledger shows it for a live run, a run's own page as its header.
 */
export function LiveRun({ id, embedded, preview, actions, onDismiss, onOutcome }: {
  id: string
  /** On a run's own page: no link to it, no dismiss. */
  embedded?: boolean
  preview?: boolean
  /** Buttons the page adds beside the card's own, such as Run again. */
  actions?: ReactNode
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

  if (live.error) return <section className="mb-6 grid gap-3 rounded-xl border border-destructive p-6"><b className="inline-flex items-center gap-2 text-destructive"><CircleAlert className="size-4" />The run could not be read</b><ErrorBox error={live.error} /></section>
  if (!run) return <section className="mb-6 flex items-center gap-2 rounded-xl border border-flame-line bg-flame-wash p-6 text-sm text-flame-ink"><Spinner />Starting the run. Centinel is fixing the selection.</section>

  const view = run.view
  const totals = view?.documents
  const scored = view?.scored ?? 0
  const total = run.document_count
  const elapsed = running ? now - Date.parse(run.created_at) : run.duration_ms ?? 0
  const rate = scored && elapsed > 0 ? scored / elapsed : null
  const left = rate ? (total - scored) / rate : null
  const questions = run.effective_questions?.length ? run.effective_questions : run.questions
  const toExclude = run.preview?.affected_documents || 0
  const failed = run.status === 'failed' || run.status === 'interrupted'
  const canCommit = !preview && run.status === 'completed' && toExclude > 0
  const go = (outcome: ResultOutcome) => onOutcome ? onOutcome(outcome) : !preview && navigate({ to: '/runs', search: { run: id, page: 1, outcome } })
  const failureGroups = groupErrors(failures.data?.results || [])
  const state = running
    ? { text: 'Scoring now', tone: 'text-flame-ink', mark: <span className="size-2 rounded-full bg-flame shadow-[0_0_0_4px_var(--flame-halo)]" /> }
    : failed ? { text: 'Stopped', tone: 'text-destructive', mark: <CircleAlert className="size-3.5" /> }
      : run.status === 'committed' ? { text: 'Committed', tone: 'text-moss', mark: <ShieldCheck className="size-3.5" /> }
        : { text: preview ? 'Preview finished' : 'Finished', tone: 'text-moss', mark: <CircleCheck className="size-3.5" /> }
  const facts = [seconds(elapsed), run.cost_usd != null && money(run.cost_usd), run.input_tokens && `${compact(run.input_tokens)} tokens`].filter(Boolean).join(' · ')

  return <section aria-live="polite" className={`mb-6 grid gap-5 rounded-xl border p-6 ${running ? 'border-flame-line bg-flame-wash' : failed ? 'border-destructive-line bg-background' : 'bg-background'}`}>
    <div className="flex flex-wrap items-start justify-between gap-6">
      <div className="grid min-w-0 flex-1 gap-1.5">
        <span className={`inline-flex items-center gap-2 text-xs font-semibold uppercase tracking-[0.08em] ${state.tone}`}>{state.mark}{state.text} · {facts}</span>
        <h2 className="text-[22px] leading-7 font-semibold tracking-[-0.01em]">{asked(run)}</h2>
        <span className="text-sm text-muted-foreground">{plural(total, 'document')} · {plural(run.questions.length, 'question')} · {run.model} · as of {run.evaluation_date}</span>
        {embedded && <span className="font-mono text-xs text-muted-foreground">{run.id}</span>}
      </div>
      <div className="flex shrink-0 items-center gap-2">
        {actions}
        {canCommit && <Button onClick={() => commit.mutate()} disabled={commit.isPending}>{commit.isPending ? <Spinner /> : <ShieldCheck />}Commit {plural(toExclude, 'exclusion')}</Button>}
        {!embedded && !preview && <Button variant="outline" className="bg-background" onClick={() => navigate({ to: '/runs', search: { run: id, page: 1, outcome: '' } })}>{running ? 'Open' : 'Every result'}<ArrowRight /></Button>}
        {!embedded && onDismiss && !running && <Button variant="ghost" size="icon" aria-label="Dismiss" onClick={onDismiss}><X /></Button>}
      </div>
    </div>

    <div className="grid gap-3">
      <div className="flex h-2.5 overflow-hidden rounded-full bg-track [&_i]:block [&_i]:h-full" role="progressbar" aria-valuemin={0} aria-valuemax={total} aria-valuenow={scored}>
        {totals && segments.map(segment => totals[segment.key] ? <i key={segment.key} className={segment.colour} style={{ width: `${(totals[segment.key] ?? 0) / Math.max(1, total) * 100}%` }} /> : null)}
        {running && <i className="animate-pulse bg-dot" style={{ width: `${(view?.in_flight?.length || 0) / Math.max(1, total) * 100}%` }} />}
      </div>
      <div className="flex flex-wrap items-center gap-x-6 gap-y-2">
        {segments.map(segment => 'quiet' in segment && !totals?.[segment.key] ? null : <Legend key={segment.key} colour={segment.colour} count={totals?.[segment.key] ?? 0} label={segment.label} onClick={() => go(segment.outcome)} />)}
        <Legend colour="bg-moss" count={totals?.tagged ?? 0} label="tagged" onClick={() => go('tag')} />
        {!!totals?.sampled && <span className="text-[13px] text-muted-foreground">{number(totals.sampled)} sampled</span>}
        <span className="ml-auto text-[13px] text-muted-foreground">{number(scored)} of {number(total)}{running && left != null ? ` · ~${seconds(left)} left` : ''}{running ? ` · ${number(view?.in_flight?.length ?? 0)} in flight to Jev` : ''}</span>
      </div>
    </div>

    {running && <div className="grid gap-6 lg:grid-cols-[minmax(0,1fr)_minmax(0,1.4fr)]">
      <Ledger title={`Out to Jev now · ${view?.in_flight?.length || 0}`} empty={scored ? 'Waiting for the next documents…' : 'Sending the first documents…'}>
        {(view?.in_flight || []).map(flight => <li key={`${flight.source}:${flight.resource}`}>
          <Spinner /><span className="min-w-0 flex-1 truncate" title={flight.resource}>{tail(flight.resource)}</span>
          {flight.attempt > 1 && <span className="text-flame-ink">request {flight.attempt}</span>}
          <span className="font-mono text-muted-foreground">{seconds(now - flight.started_ms)}</span>
        </li>)}
      </Ledger>
      <Ledger title="Latest answers" empty="No answers yet. The first ones usually take a few seconds.">
        {(view?.recent || []).map(result => <li key={`${result.source}:${result.resource}:${result.derived_sha}`}>
          <DecisionBadge decision={decisionOf(result)} />
          <DocumentLink doc={result} className="min-w-0 flex-1 truncate hover:underline">{tail(result.resource)}</DocumentLink>
          <span className="min-w-0 flex-1 truncate font-mono text-muted-foreground" title={answerSummary(questions, result)}>{answerSummary(questions, result)}</span>
        </li>)}
      </Ledger>
    </div>}

    {!running && <p className="text-sm text-muted-foreground">
      {preview ? 'A preview: the scores are below. Nothing was saved, so it cannot be committed.'
        : run.status === 'committed' ? `Its exclusions are committed.${toExclude ? ` A policy change since then would exclude ${plural(toExclude, 'more document')}.` : ''}`
          : toExclude ? `Committing takes ${plural(toExclude, 'document')} out of search and future embedding. The archive does not change, and any document can be restored.`
            : 'No document reaches an exclude threshold. Nothing needs committing.'}
      {commit.isSuccess && ' Committed.'}
    </p>}
    {commit.error && <ErrorBox error={commit.error} />}

    {!running && errors > 0 && <div className="grid gap-1.5 border-t pt-4 text-destructive">
      <b className="text-[13px]">{plural(errors, 'document')} failed</b>
      {failures.isLoading && <span className="text-xs text-muted-foreground">Reading the errors…</span>}
      {failureGroups.map(([message, count]) => <span key={message} className="text-xs wrap-anywhere"><b className="font-mono">{number(count)}×</b> {message}</span>)}
    </div>}
  </section>
}

/** One decision in the legend: its colour, its count, and a click that filters the results to it. */
function Legend({ colour, count, label, onClick }: { colour: string; count: number; label: string; onClick: () => void }) {
  return <button type="button" disabled={!count} onClick={onClick} className="inline-flex items-center gap-2 text-[13px] enabled:hover:underline disabled:opacity-45">
    <i className={`size-2.5 rounded-[2px] ${colour}`} /><b className="font-semibold">{number(count)}</b>{label}
  </button>
}

function Ledger({ title, empty, children }: { title: string; empty: string; children: ReactNode[] }) {
  return <div className="grid content-start gap-1">
    <span className="mb-1 text-[11px] font-semibold uppercase tracking-[0.08em] text-muted-foreground">{title}</span>
    <ul className="grid text-xs [&_li]:flex [&_li]:min-w-0 [&_li]:items-center [&_li]:gap-2 [&_li]:border-b [&_li]:py-1.5">
      {children}
      {!children.length && <li className="text-muted-foreground">{empty}</li>}
    </ul>
  </div>
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
