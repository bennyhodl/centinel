import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import React, { useState } from 'react'
import { ArrowDown, ArrowUp, ArrowUpDown, ChevronDown, ChevronLeft, ChevronRight, ExternalLink } from 'lucide-react'
import type { Question, ResultOutcome, Run, RunDetailQuery, RunResult } from './api'
import { Button } from '@/components/ui/button'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { compact, number, seconds, tail } from './format'
import { decisionOf, isChoice, optionScores, policyShort, sortKeys, tagsOf } from './policy'
import { DecisionBadge, DocumentLink, Empty, ScoreBar } from './ui'

const outcomeTabs: Array<{ value: ResultOutcome; label: string; count: (run: Run) => number }> = [
  { value: '', label: 'All', count: run => run.view?.scored ?? 0 },
  { value: 'exclude', label: 'Exclude', count: run => run.view?.documents.excluded ?? 0 },
  { value: 'review', label: 'Review', count: run => run.view?.documents.review ?? 0 },
  { value: 'keep', label: 'Keep', count: run => run.view?.documents.kept ?? 0 },
  { value: 'tag', label: 'Tagged', count: run => run.view?.documents.tagged ?? 0 },
  { value: 'error', label: 'Errors', count: run => run.view?.documents.errors ?? 0 },
]

/**
 * One page of a run's results. The server filters and sorts, so a run over the whole
 * corpus never comes to the browser at once. Every column header sorts; the Sort
 * control also reaches each option of a choice. A row opens to show every probability.
 */
export function ResultsSection({ run, questions, view, setView, loading }: {
  run: Run
  questions: Question[]
  view: RunDetailQuery
  setView: (view: RunDetailQuery) => void
  loading?: boolean
}) {
  const [open, setOpen] = useState<Set<string>>(new Set())
  const total = run.view?.result_total ?? run.results.length
  const pages = Math.max(1, Math.ceil(total / view.page_size))
  const keys = sortKeys(questions)
  const sortBy = (key: string) => setView({
    ...view, page: 1, sort: key,
    direction: view.sort === key ? (effectiveDirection(view) === 'asc' ? 'desc' : 'asc') : defaultDirection(key),
  })
  const toggle = (key: string) => setOpen(current => {
    const next = new Set(current)
    if (next.has(key)) next.delete(key)
    else next.add(key)
    return next
  })
  const columns = 3 + questions.length

  return <section className="grid gap-3">
    <div className="flex flex-wrap items-center justify-between gap-3">
      <div className="flex flex-wrap gap-1 [&_button]:inline-flex [&_button]:items-center [&_button]:gap-2 [&_button]:rounded-full [&_button]:border [&_button]:px-3 [&_button]:py-1 [&_button]:text-xs [&_button]:hover:bg-accent" role="tablist" aria-label="Filter by decision">
        {outcomeTabs.map(tab => <button type="button" role="tab" aria-selected={view.outcome === tab.value} key={tab.label} className={`${view.outcome === tab.value ? 'bg-primary text-primary-foreground' : ''}`} onClick={() => setView({ ...view, outcome: tab.value, page: 1 })}>
          {tab.label}<b>{number(tab.count(run))}</b>
        </button>)}
      </div>
      <div className="flex items-center gap-2 text-xs text-muted-foreground">
        <span>Sort</span>
        <Select value={view.sort || 'none'} onValueChange={key => key === 'none' ? setView({ ...view, sort: '', direction: '', page: 1 }) : setView({ ...view, sort: key, direction: defaultDirection(key), page: 1 })}>
          <SelectTrigger aria-label="Sort results by"><SelectValue /></SelectTrigger>
          <SelectContent><SelectItem value="none">Order scored</SelectItem>{keys.map(([key, label]) => <SelectItem value={key} key={key}>{label}</SelectItem>)}</SelectContent>
        </Select>
        {view.sort && <Button size="sm" variant="secondary" onClick={() => setView({ ...view, direction: effectiveDirection(view) === 'asc' ? 'desc' : 'asc', page: 1 })}>{effectiveDirection(view) === 'asc' ? <><ArrowUp />Lowest first</> : <><ArrowDown />Highest first</>}</Button>}
      </div>
    </div>

    <div className={`overflow-x-auto overflow-auto rounded-lg border bg-card [&_table]:min-w-[640px] [&_th]:p-0 ${loading ? 'opacity-65' : ''}`}>
      <Table>
        <TableHeader><TableRow>
          <TableHead className="w-8 [&_svg]:size-4" aria-label="Open" />
          <SortHeader label="Document" sortKey="resource" view={view} onSort={sortBy} />
          <SortHeader label="Decision" sortKey="decision" view={view} onSort={sortBy} />
          {questions.map(question => <SortHeader key={question.id} label={question.id} note={isChoice(question) ? ((question.options || []).some(option => option.action === 'exclude') ? 'winner · junk' : 'winner') : `≥ ${question.threshold.toFixed(2)}`} sortKey={question.id} view={view} onSort={sortBy} />)}
        </TableRow></TableHeader>
        <TableBody>
          {run.results.map(result => {
            const key = `${result.source}:${result.resource}:${result.derived_sha}`
            const decision = decisionOf(result)
            const isOpen = open.has(key)
            return <React.Fragment key={key}>
              <TableRow className={`cursor-pointer hover:bg-muted ${isOpen ? 'bg-muted' : ''}`} onClick={() => toggle(key)}>
                <TableCell className="w-8 [&_svg]:size-4"><ChevronDown className={isOpen ? 'rotate-180' : ''} /></TableCell>
                <TableCell className="min-w-36 max-w-96 [&_b]:block [&_b]:truncate [&_small]:block [&_small]:truncate [&_small]:text-xs [&_small]:text-muted-foreground">
                  <b title={result.resource}>{tail(result.resource)}</b>
                  <small>{result.source}{result.sampled ? ` · sampled ${compact(result.sampled.sent_chars)} of ${compact(result.sampled.total_chars)} chars` : ''}{(result.attempts || 0) > 1 ? ` · ${result.attempts} requests` : ''}</small>
                </TableCell>
                <TableCell className="min-w-24"><DecisionBadge decision={decision} />{tagsOf(result).map(tag => <span className="ml-1 inline-flex rounded-full bg-secondary px-2 py-0.5 font-mono text-xs" key={tag}>{tag}</span>)}</TableCell>
                {result.error
                  ? <TableCell className="max-w-96 truncate text-xs text-destructive" colSpan={questions.length}>{result.error}</TableCell>
                  : questions.map(question => <TableCell key={question.id}><AnswerCell question={question} result={result} /></TableCell>)}
              </TableRow>
              {isOpen && <TableRow className="bg-muted [&_td]:px-4 [&_td]:pb-4"><TableCell colSpan={columns}><ResultDetail result={result} questions={questions} /></TableCell></TableRow>}
            </React.Fragment>
          })}
        </TableBody>
      </Table>
      {!run.results.length && <Empty>{view.outcome ? 'No document has this decision.' : run.status === 'running' ? 'No answers yet.' : 'This run has no results.'}</Empty>}
    </div>

    {pages > 1 && <div className="flex h-14 items-center justify-end gap-3 border-t px-4 text-xs"><Button variant="secondary" size="sm" disabled={view.page === 1} onClick={() => setView({ ...view, page: view.page - 1 })}><ChevronLeft />Previous</Button><span>Page {view.page} of {pages} · {number(total)} documents</span><Button variant="secondary" size="sm" disabled={view.page >= pages} onClick={() => setView({ ...view, page: view.page + 1 })}>Next<ChevronRight /></Button></div>}
  </section>
}

const defaultDirection = (key: string): 'asc' | 'desc' => key === 'resource' || key === 'decision' ? 'asc' : 'desc'
const effectiveDirection = (view: RunDetailQuery) => view.direction || defaultDirection(view.sort)

function SortHeader({ label, note, sortKey, view, onSort }: { label: string; note?: string; sortKey: string; view: RunDetailQuery; onSort: (key: string) => void }) {
  const active = view.sort === sortKey
  const direction = effectiveDirection(view)
  const Icon = !active ? ArrowUpDown : direction === 'asc' ? ArrowUp : ArrowDown
  return <TableHead aria-sort={active ? (direction === 'asc' ? 'ascending' : 'descending') : 'none'}>
    <button type="button" className={`flex w-full items-end justify-between gap-2 p-3 text-left text-xs font-medium [&_small]:block [&_small]:text-muted-foreground [&_svg]:size-3 ${active ? 'bg-parchment font-semibold text-foreground [&_svg]:text-flame-ink' : ''}`} onClick={() => onSort(sortKey)} title={`Sort by ${label}`}>
      <span>{label}{note && <small>{note}</small>}</span><Icon />
    </button>
  </TableHead>
}

function toneOf(result: RunResult, question: Question) {
  const outcome = result.outcomes?.[question.id]
  return outcome?.excluded ? 'exclude' : outcome?.review ? 'review' : outcome?.tags?.length ? 'tag' : ''
}

function AnswerCell({ question, result }: { question: Question; result: RunResult }) {
  const tone = toneOf(result, question)
  if (!isChoice(question)) {
    const score = result.answers[question.id]
    return score == null ? <span className="text-muted-foreground">—</span> : <ScoreBar value={score} threshold={question.threshold} review={question.review} tone={tone} />
  }
  const outcome = result.outcomes?.[question.id]
  const top = outcome?.top || result.choices?.[question.id]?.choice
  if (!top) return <span className="text-muted-foreground">—</span>
  const option = question.options?.find(candidate => candidate.id === top)
  const score = result.answers[`${question.id}:${top}`] ?? 0
  return <span className="inline-grid grid-cols-[auto_auto] gap-x-2 rounded bg-secondary px-2 py-1 [&_small]:col-span-full [&_small]:text-xs">
    <span className="max-w-32 truncate font-mono text-xs font-medium">{top}</span>
    <span className="font-mono text-xs text-muted-foreground">{score.toFixed(2)}</span>
    {outcome?.exclusion != null && option?.action !== 'exclude' && outcome.exclusion >= 0.05 && <small>junk {outcome.exclusion.toFixed(2)}</small>}
  </span>
}

function ResultDetail({ result, questions }: { result: RunResult; questions: Question[] }) {
  return <div className="grid gap-3 [&_footer]:flex [&_footer]:flex-wrap [&_footer]:items-center [&_footer]:gap-3 [&_footer]:text-xs [&_footer]:text-muted-foreground">
    {result.error && <p className="wrap-anywhere text-xs text-destructive">{result.error}</p>}
    {!result.error && <div className="grid grid-cols-[repeat(auto-fit,minmax(280px,1fr))] gap-3">{questions.map(question => {
      const outcome = result.outcomes?.[question.id]
      return <div className="grid content-start gap-2 rounded-md border bg-card p-3 [&_h4]:flex [&_h4]:flex-col [&_h4]:font-mono [&_h4]:text-xs [&_h4]:font-semibold [&_small]:font-normal [&_small]:text-muted-foreground [&_p]:text-xs" key={question.id}>
        <h4>{question.id}<small>{policyShort(question)}</small></h4>
        {isChoice(question)
          ? <ul className="grid gap-1 [&_li]:grid [&_li]:grid-cols-[140px_minmax(0,1fr)] [&_li]:items-center [&_li]:gap-2 [&_li]:text-xs">{optionScores(question, result).map(({ option, score }) => <li key={option.id} className={`${option.id === outcome?.top ? 'font-semibold' : ''}`}>
              <span className="truncate font-mono [&_em]:ml-1 [&_em]:text-xs [&_em]:not-italic [&_em]:text-muted-foreground">{option.id}<em>{option.action === 'keep' ? '' : option.action}</em></span>
              <ScoreBar value={score} threshold={option.action === 'keep' ? undefined : question.threshold} tone={option.id === outcome?.top ? toneOf(result, question) || 'won' : ''} />
            </li>)}</ul>
          : <ScoreBar value={result.answers[question.id] ?? 0} threshold={question.threshold} review={question.review} tone={toneOf(result, question)} />}
        {outcome?.exclusion != null && <p className="text-muted-foreground">Exclude options together: <b>{outcome.exclusion.toFixed(2)}</b></p>}
        {result.choices?.[question.id]?.confidence != null && <p className="text-muted-foreground">Jev confidence {result.choices[question.id].confidence!.toFixed(2)}</p>}
      </div>
    })}</div>}
    <footer>
      <span>{seconds(result.duration_ms)}</span>
      {result.attempts != null && <span>{number(result.attempts)} {result.attempts === 1 ? 'request' : 'requests'}</span>}
      {result.sampled && <span>Jev saw {compact(result.sampled.sent_chars)} of {compact(result.sampled.total_chars)} characters</span>}
      <span className="font-mono">{result.derived_sha.slice(0, 12)}</span>
      <DocumentLink doc={result} className="ml-auto inline-flex items-center gap-1 font-medium hover:underline [&_svg]:size-3">Open document<ExternalLink /></DocumentLink>
    </footer>
  </div>
}
