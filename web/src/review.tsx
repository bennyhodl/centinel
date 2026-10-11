import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { useEffect, useMemo, useState, type ReactNode } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { api, type Evaluation, type Question, type QuestionEvaluation, type ReviewCandidate } from './api'
import { queries } from './queries'
import { Button } from '@/components/ui/button'
import { Skeleton } from '@/components/ui/skeleton'
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle, SheetTrigger } from '@/components/ui/sheet'
import { characters, number, plural, tail } from './format'
import { isChoice } from './policy'
import { type Answers, buildReview, decidingScore, modelAnswers, swipe, unsure } from './review-logic'
import { DocumentLink, Empty, ErrorBox, PageHeader, SectionRule, shortSha, Spinner } from './ui'

/** How many cards Review takes at once. The route's loader warms the same page. */
export const REVIEW_QUEUE = 20

const detail = 'See how the model read a document, and say where it is wrong.'

/**
 * One document at a time: its text on the left, the model's answer to every question on
 * the right, each one yours to overturn. The keyboard does the work: 1–9 picks a
 * question, Y and N answer it, ← calls it junk, → keeps it, Enter records the card as it
 * stands. Every card is one line in the reviews ledger and acts on search at once.
 */
export function Review() {
  const client = useQueryClient()
  const questions = useQuery(queries.questions())
  const queue = useQuery(queries.reviewQueue(REVIEW_QUEUE))
  const evaluation = useQuery(queries.evaluation())
  const [cursor, setCursor] = useState(0)
  const [last, setLast] = useState('')
  const candidate = queue.data?.documents[cursor]
  const saved = questions.data?.questions || []

  const advance = () => {
    if (queue.data && cursor + 1 < queue.data.documents.length) setCursor(cursor + 1)
    else { setCursor(0); queue.refetch() }
  }
  const submit = useMutation({
    mutationFn: api.review,
    onSuccess: report => {
      setLast(report.usage_changed ? (report.excluded ? 'Excluded from search.' : 'Restored to search.') : report.tags.length ? `Recorded. Tags: ${report.tags.join(', ')}` : 'Recorded.')
      client.invalidateQueries({ queryKey: ['evaluation'] })
      client.invalidateQueries({ queryKey: ['corpus'] })
      advance()
    },
  })

  return <div className="flex flex-col xl:h-[calc(100svh-5.25rem)]">
    <PageHeader title="Review" detail={detail}>
      {evaluation.data && evaluation.data.reviews > 0 && <Agreement evaluation={evaluation.data} />}
    </PageHeader>
    {queue.error ? <ErrorBox error={queue.error} /> : questions.error ? <ErrorBox error={questions.error} /> : !queue.data || !questions.data ? <div className="flex items-center gap-2 py-6 text-sm text-muted-foreground"><Spinner /> Reading the queue…</div>
      : !candidate ? <Empty>{queue.data.scored === 0 ? 'Nothing is scored yet. Run `centinel classify`, or a run from the Classify page.' : 'Everything scored has been reviewed.'}</Empty>
      : <Card key={`${candidate.source}:${candidate.resource}:${candidate.derived_sha}`} candidate={candidate} questions={saved} evaluation={evaluation.data}
          busy={submit.isPending} error={submit.error} last={last}
          left={Math.max(0, queue.data.scored - queue.data.reviewed - cursor)}
          onSubmit={answers => submit.mutate(buildReview(candidate, saved, answers))} />}
  </div>
}

function Card({ candidate, questions, evaluation, busy, error, last, left, onSubmit }: {
  candidate: ReviewCandidate
  questions: Question[]
  evaluation: Evaluation | undefined
  busy: boolean
  error: Error | null
  last: string
  left: number
  onSubmit: (answers: Answers) => void
}) {
  const text = useQuery(queries.read(candidate))
  const model = useMemo(() => modelAnswers(questions, candidate), [questions, candidate])
  const [edits, setEdits] = useState<Answers>({})
  const [active, setActive] = useState(() => Math.max(0, questions.findIndex(question => unsure(question, candidate))))
  const answers: Answers = { ...model, ...edits }
  const set = (id: string, value: boolean | string) => setEdits(current => ({ ...current, [id]: value }))
  const junk = swipe(questions, candidate, answers, false)

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null
      if (busy || event.metaKey || event.ctrlKey || event.altKey) return
      if (target?.closest('input, textarea, [role="dialog"]')) return
      const key = event.key.toLowerCase()
      const question = questions[active]
      if (key === 'arrowleft' && junk) { event.preventDefault(); onSubmit(junk) }
      else if (key === 'arrowright') { const kept = swipe(questions, candidate, answers, true); if (kept) { event.preventDefault(); onSubmit(kept) } }
      else if (key === 'enter') { event.preventDefault(); onSubmit(answers) }
      else if (/^[1-9]$/.test(key) && Number(key) <= questions.length) { event.preventDefault(); setActive(Number(key) - 1) }
      else if ((key === 'y' || key === 'n') && question && !isChoice(question)) { event.preventDefault(); set(question.id, key === 'y') }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  })

  const facts = [candidate.source, address(candidate.resource), text.data?.kind.toUpperCase(), characters(candidate.chars), shortSha(candidate.blob_sha)]
  return <div className="grid min-h-0 flex-1 items-stretch gap-10 xl:grid-cols-[minmax(0,1fr)_340px]">
    <article className="flex min-h-0 min-w-0 flex-col gap-3.5">
      <header className="grid gap-1 border-b border-foreground pb-3">
        <h2 className="text-xl leading-[26px] font-semibold tracking-[-0.01em] wrap-anywhere [&_a]:hover:underline"><DocumentLink doc={candidate}>{candidate.title || tail(candidate.resource)}</DocumentLink></h2>
        <p className="flex flex-wrap items-center gap-x-2 text-xs text-muted-foreground">
          <span className="wrap-anywhere">{facts.filter(Boolean).join(' · ')}</span>
          {candidate.excluded && <span className="rounded-sm bg-flame-soft px-1.5 py-px font-semibold text-flame-ink">Excluded from search</span>}
          {candidate.reviewed && <span className="rounded-sm bg-muted px-1.5 py-px">Reviewed before</span>}
        </p>
      </header>
      <div className="max-h-[55svh] min-h-0 flex-1 overflow-y-auto pr-3 pb-6 xl:max-h-none">
        {text.error ? <ErrorBox error={text.error} />
          : text.data ? <>
            <p className="max-w-[72ch] font-reading text-base leading-[26px] whitespace-pre-wrap wrap-anywhere">{text.data.text}</p>
            {text.data.truncated && <p className="mt-4 text-xs text-muted-foreground">The first {characters(text.data.chars)} of {characters(text.data.total_chars)}. <DocumentLink doc={candidate} className="text-foreground underline underline-offset-2">Open the document</DocumentLink> for the rest.</p>}
          </>
          : <div className="flex items-center gap-2 py-6 text-sm text-muted-foreground"><Spinner /> Reading…</div>}
      </div>
    </article>
    <aside className="flex min-h-0 flex-col">
      <SectionRule aside={`${number(left)} unreviewed`}>Model vs you</SectionRule>
      <div className="-ml-3 min-h-0 flex-1 overflow-y-auto pl-3">
        {questions.map((question, index) => <Verdict key={question.id} question={question} candidate={candidate} answer={answers[question.id]} model={model[question.id]}
          agreement={evaluation?.questions.find(scored => scored.id === question.id)} active={index === active}
          onPick={() => setActive(index)} onAnswer={value => set(question.id, value)} />)}
      </div>
      <footer className="grid gap-2.5 pt-4">
        {error && <ErrorBox error={error} />}
        {last && <p className="text-xs text-muted-foreground">{last}</p>}
        <div className="flex gap-2">
          {junk && <Button variant="outline" className="h-10 px-3.5" disabled={busy} onClick={() => onSubmit(junk)}>← Junk</Button>}
          <Button className="h-10 flex-1 font-semibold" disabled={busy} onClick={() => onSubmit(answers)}>{busy && <Spinner />}Record & next <Key>↵</Key></Button>
        </div>
        <p className="text-[11px] text-muted-foreground">1–9 picks a question · Y or N answers it{junk ? ' · → keeps the page' : ''}</p>
      </footer>
    </aside>
  </div>
}

/**
 * One question: the model's call and how sure it was, then the person's answer. Amber when
 * the model is unsure, because that is where a person's answer teaches it most.
 */
function Verdict({ question, candidate, answer, model, agreement, active, onPick, onAnswer }: {
  question: Question
  candidate: ReviewCandidate
  answer: boolean | string | undefined
  model: boolean | string | undefined
  agreement: QuestionEvaluation | undefined
  active: boolean
  onPick: () => void
  onAnswer: (value: boolean | string) => void
}) {
  const doubt = unsure(question, candidate)
  const top = candidate.outcomes[question.id]?.top
  const shown = isChoice(question) ? (top ? candidate.classifications[`${question.id}:${top}`] : undefined) : decidingScore(question, candidate)
  const call = shown == null ? 'not asked'
    : isChoice(question) ? `${top} ${odds(shown)}${doubt ? ' · unsure' : ''}`
    : `${odds(shown)} · ${doubt ? 'unsure' : model ? 'yes' : 'no'}`
  const disagree = answer != null && model != null && answer !== model
  return <section onPointerDown={onPick} className={`relative grid gap-2.5 border-b border-rule py-4 ${active ? 'before:absolute before:inset-y-4 before:-left-3 before:w-0.5 before:rounded-full before:bg-flame' : ''}`}>
    <div className="flex items-baseline justify-between gap-3">
      <h3 className="text-sm font-semibold" title={question.instructions}>{label(question.id)}</h3>
      <span className={`shrink-0 font-mono text-[13px] ${doubt ? 'font-semibold text-flame-ink' : shown == null ? 'text-muted-foreground' : ''}`}>{call}</span>
    </div>
    <div className="h-1.5 rounded-full bg-track"><div className={`h-full rounded-full ${doubt ? 'bg-flame' : 'bg-foreground'}`} style={{ width: `${(shown ?? 0) * 100}%` }} /></div>
    {isChoice(question)
      ? <div className="flex flex-wrap gap-1.5">{(question.options || []).map(option => {
          const p = candidate.classifications[`${question.id}:${option.id}`]
          return <Choice key={option.id} on={answer === option.id} onClick={() => onAnswer(option.id)} title={option.description}>{option.id}{p != null && ` ${odds(p)}`}</Choice>
        })}</div>
      : <div className="grid grid-cols-2 gap-1.5">
          <Choice wide on={answer === true} onClick={() => onAnswer(true)}>Yes{active && <Key>Y</Key>}</Choice>
          <Choice wide on={answer === false} onClick={() => onAnswer(false)}>No{active && <Key>N</Key>}</Choice>
        </div>}
    {disagree ? <p className="text-xs font-medium text-flame-ink">You disagree with the model.</p>
      : agreement?.compared && agreement.agreement != null ? <p className="text-xs text-muted-foreground">You agreed with it {number(Math.round(agreement.agreement * agreement.compared))} of {plural(agreement.compared, 'time')}.</p>
      : null}
  </section>
}

/** An answer to press: inked when it is the one on the card. */
function Choice({ on, wide, title, onClick, children }: { on: boolean; wide?: boolean; title?: string; onClick: () => void; children: ReactNode }) {
  return <button type="button" aria-pressed={on} title={title} onClick={onClick}
    className={`inline-flex items-center justify-center gap-1.5 transition-colors duration-(--motion-micro) ${wide ? 'h-8 rounded-md text-[13px] font-semibold' : 'h-6 rounded-sm px-2 text-xs'} ${on ? 'bg-primary font-semibold text-primary-foreground' : 'text-ink-soft shadow-[inset_0_0_0_1px_var(--input)] hover:bg-hover'}`}>
    {children}
  </button>
}

function Key({ children }: { children: ReactNode }) {
  return <kbd className="font-mono text-[11px] font-normal opacity-60">{children}</kbd>
}

/** A probability as the panel prints it: `.58`, and `1.00` when certain. */
const odds = (value: number) => value.toFixed(2).replace(/^0\./, '.')
const label = (id: string) => id.charAt(0).toUpperCase() + id.slice(1).replace(/_/g, ' ')

/** The part of an address a reader tells documents apart by: the path, not the host the source already names. */
function address(resource: string) {
  try {
    const url = new URL(resource)
    return decodeURIComponent(url.pathname + url.search)
  } catch { return resource }
}

/** How the model is doing against the people, from the same ledger the cards write. */
function Agreement({ evaluation }: { evaluation: Evaluation }) {
  const percent = (value?: number | null) => value == null ? '—' : `${Math.round(value * 100)}%`
  return <Sheet>
    <SheetTrigger asChild><Button variant="outline" size="sm">Agreement · {plural(evaluation.reviews, 'review')}</Button></SheetTrigger>
    <SheetContent side="right" className="flex w-full flex-col gap-0 p-0 sm:max-w-[720px]">
      <SheetHeader className="border-b px-6 pt-6 pb-5">
        <SheetTitle className="font-serif text-[30px] leading-8 font-normal">Agreement</SheetTitle>
        <SheetDescription>{number(evaluation.reviews)} reviews of {number(evaluation.documents)} documents. `centinel evaluate` has the detail.</SheetDescription>
      </SheetHeader>
      <div className="overflow-auto px-2"><Table className="[&_b]:block [&_b]:font-mono [&_b]:text-xs"><TableHeader><TableRow><TableHead>Question</TableHead><TableHead>Compared</TableHead><TableHead>Agree</TableHead><TableHead>Threshold</TableHead><TableHead>Would agree most at</TableHead><TableHead>Precision</TableHead><TableHead>Recall</TableHead></TableRow></TableHeader><TableBody>
        {evaluation.questions.map(question => <TableRow key={question.id}><TableCell><b>{question.id}</b><small className="text-muted-foreground">{question.kind}</small></TableCell><TableCell>{question.compared ? number(question.compared) : '—'}</TableCell><TableCell>{percent(question.agreement)}</TableCell><TableCell>{question.threshold.toFixed(2)}</TableCell><TableCell>{question.suggested_threshold == null ? '—' : question.suggested_threshold.toFixed(2)}</TableCell><TableCell>{percent(question.precision)}</TableCell><TableCell>{percent(question.recall)}</TableCell></TableRow>)}
      </TableBody></Table></div>
      {Object.keys(evaluation.proposed).length > 0 && <p className="border-t px-6 py-3 text-xs text-muted-foreground">Proposed tags: {Object.entries(evaluation.proposed).sort((a, b) => b[1] - a[1]).map(([name, count]) => `${name} ×${count}`).join(' · ')}</p>}
    </SheetContent>
  </Sheet>
}

/** Review before its queue arrives: the real header, the document and its panel masked. */
export function ReviewSkeleton() {
  return <div aria-busy className="flex flex-col xl:h-[calc(100svh-5.25rem)]">
    <PageHeader title="Review" detail={detail} />
    <div className="grid min-h-0 flex-1 gap-10 xl:grid-cols-[minmax(0,1fr)_340px]">
      <div className="grid content-start gap-3.5">
        <div className="grid gap-1.5 border-b border-foreground pb-3">
          <Skeleton mask="Community Foundation Tampa Bay Speaker Series" className="text-xl font-semibold" />
          <Skeleton mask="cftampabay · /events/outstanding-speaker-series · HTML · 6.7k chars · 470278a" className="text-xs" />
        </div>
        {Array.from({ length: 8 }, (_, i) => <Skeleton key={i} className="h-4" />)}
      </div>
      <div className="grid content-start">
        <SectionRule>Model vs you</SectionRule>
        {Array.from({ length: 3 }, (_, i) => <div key={i} className="grid gap-2.5 border-b border-rule py-4"><Skeleton className="h-4 w-40" /><Skeleton className="h-1.5" /><Skeleton className="h-8" /></div>)}
      </div>
    </div>
  </div>
}
