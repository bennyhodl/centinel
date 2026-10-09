import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { useEffect, useMemo, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Check, Keyboard, ThumbsDown, ThumbsUp } from 'lucide-react'
import { api, type Evaluation, type Question, type ReviewCandidate } from './api'
import { queries } from './queries'
import { Button } from '@/components/ui/button'
import { Skeleton } from '@/components/ui/skeleton'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { characters, number, tail } from './format'
import { isChoice } from './policy'
import { type Answers, buildReview, gateOf, gateOption, modelAnswers } from './review-logic'
import { DocumentLink, Empty, ErrorBox, PageHeader, Segmented, Spinner } from './ui'

/** How many cards Review takes at once. The route's loader warms the same page. */
export const REVIEW_QUEUE = 20

/**
 * One document at a time. The text on the left, the policy's decisions on the right, and
 * the keyboard does the work: → record, ← junk, Enter records the card as it stands, s
 * skips. Every card is one line in the reviews ledger and acts on search at once.
 */
export function Review() {
  const client = useQueryClient()
  const questions = useQuery(queries.questions())
  const [source, setSource] = useState('')
  const [includeReviewed, setIncludeReviewed] = useState(false)
  const queue = useQuery(queries.reviewQueue(source, includeReviewed, REVIEW_QUEUE))
  const sources = useQuery(queries.corpus({}, 1, 1))
  const evaluation = useQuery(queries.evaluation())
  const [cursor, setCursor] = useState(0)
  const [last, setLast] = useState('')
  const candidate = queue.data?.documents[cursor]
  const saved = questions.data?.questions || []
  const gate = gateOf(saved)

  const advance = () => {
    if (queue.data && cursor + 1 < queue.data.documents.length) setCursor(cursor + 1)
    else { setCursor(0); queue.refetch() }
  }
  const submit = useMutation({
    mutationFn: api.review,
    onSuccess: report => {
      setLast(report.usage_changed ? (report.excluded ? 'Excluded.' : 'Restored to search.') : report.tags.length ? `Tags: ${report.tags.join(', ')}` : 'Recorded.')
      client.invalidateQueries({ queryKey: ['evaluation'] })
      client.invalidateQueries({ queryKey: ['corpus'] })
      advance()
    },
  })

  return <>
    <PageHeader title="Review" detail="Read the document, say what it is. Right arrow for a record, left for junk, Enter to record the card as it stands.">
      <div className="grid gap-1 border-l pl-4 text-sm [&_span]:text-muted-foreground [&_b]:text-2xl"><span>In the review band</span><b>{queue.data ? number(queue.data.in_review_band) : '—'}</b></div>
      <div className="grid gap-1 border-l pl-4 text-sm [&_span]:text-muted-foreground [&_b]:text-2xl"><span>Reviewed</span><b>{queue.data ? number(queue.data.reviewed) : '—'}</b></div>
    </PageHeader>
    <div className="mb-4 flex flex-wrap items-end gap-4 text-xs">
      <label className="grid min-w-32 gap-2 text-xs font-medium [&_span]:flex [&_span]:items-center [&_span]:gap-1 [&_svg]:size-4 [&_[data-slot=select-trigger]]:w-full min-w-32"><span>Source</span><Select value={source || 'all'} onValueChange={value => { setSource(value === 'all' ? '' : value); setCursor(0) }}><SelectTrigger aria-label="Source"><SelectValue /></SelectTrigger><SelectContent><SelectItem value="all">All sources</SelectItem>{(sources.data?.sources || []).map(value => <SelectItem value={value} key={value}>{value}</SelectItem>)}</SelectContent></Select></label>
      <label className="flex items-center gap-2"><input type="checkbox" checked={includeReviewed} onChange={event => { setIncludeReviewed(event.target.checked); setCursor(0) }} /> Show reviewed documents too</label>
      <span className="text-muted-foreground">{queue.data ? `${number(queue.data.scored)} scored` : ''}{last ? ` · ${last}` : ''}</span>
    </div>
    {queue.error ? <ErrorBox error={queue.error} /> : questions.error ? <ErrorBox error={questions.error} /> : !queue.data || !questions.data ? <div className="flex items-center gap-2 py-6 text-sm text-muted-foreground"><Spinner /> Reading the queue…</div>
      : !candidate ? <Empty>{queue.data.scored === 0 ? 'Nothing is scored yet. Run `centinel classify`, or a run from the Classify page.' : 'Everything scored has been reviewed. Tick “Show reviewed documents too” to go round again.'}</Empty>
      : <Card key={`${candidate.source}:${candidate.resource}:${candidate.derived_sha}`} candidate={candidate} questions={saved} gate={gate} busy={submit.isPending} error={submit.error} position={`${cursor + 1} of ${queue.data.documents.length}`}
          onSkip={advance}
          onSubmit={(answers, proposed, note) => submit.mutate(buildReview(candidate, saved, answers, proposed, note))} />}
    {evaluation.data && evaluation.data.reviews > 0 && <Agreement evaluation={evaluation.data} />}
  </>
}

function Card({ candidate, questions, gate, busy, error, position, onSkip, onSubmit }: {
  candidate: ReviewCandidate
  questions: Question[]
  gate: Question | undefined
  busy: boolean
  error: Error | null
  position: string
  onSkip: () => void
  onSubmit: (answers: Answers, proposed: string, note: string) => void
}) {
  const text = useQuery(queries.read(candidate))
  const model = useMemo(() => modelAnswers(questions, candidate), [questions, candidate])
  const [edits, setEdits] = useState<Answers>({})
  const [proposed, setProposed] = useState('')
  const [note, setNote] = useState('')
  const answers: Answers = { ...model, ...edits }
  const set = (id: string, value: boolean | string) => setEdits(current => ({ ...current, [id]: value }))
  const swipe = (keep: boolean) => {
    if (!gate) return onSubmit(answers, proposed, note)
    onSubmit({ ...answers, [gate.id]: gateOption(gate, candidate.outcomes[gate.id]?.top, keep) }, proposed, note)
  }

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null
      if (busy || target?.tagName === 'INPUT' || target?.tagName === 'TEXTAREA') return
      if (event.key === 'ArrowRight') { event.preventDefault(); swipe(true) }
      else if (event.key === 'ArrowLeft') { event.preventDefault(); swipe(false) }
      else if (event.key === 'Enter') { event.preventDefault(); onSubmit(answers, proposed, note) }
      else if (event.key === 's') { event.preventDefault(); onSkip() }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  })

  const gateTop = gate ? candidate.outcomes[gate.id]?.top : undefined
  const gateChoice = gate ? String(answers[gate.id] ?? '') : ''
  const junk = gate ? (gate.options || []).find(option => option.id === gateChoice)?.action === 'exclude' : false
  return <div className="grid items-start gap-4 xl:grid-cols-[minmax(0,1fr)_340px]">
    <article className="min-w-0 rounded-lg border bg-card [&_pre]:whitespace-pre-wrap [&_pre]:wrap-anywhere [&_pre]:p-6 [&_pre]:font-sans [&_pre]:text-sm [&_pre]:leading-7 [&_pre]:max-h-[72vh] [&_pre]:overflow-auto">
      <div className="flex min-h-11 flex-wrap items-center gap-4 border-b px-4 py-2 font-mono text-xs text-muted-foreground"><span>{candidate.source}</span><span>{characters(candidate.chars)}</span><span>{candidate.excluded ? 'excluded now' : 'included now'}</span>{candidate.review_band && <span className="inline-flex rounded-full bg-secondary px-2 py-0.5 text-xs">review band</span>}{candidate.reviewed && <span className="inline-flex rounded-full bg-secondary px-2 py-0.5 text-xs">reviewed before</span>}<span className="flex-1" /><span>{position}</span></div>
      <h2 className="mx-6 mt-4 text-xl font-semibold [&_a]:hover:underline"><DocumentLink doc={candidate}>{candidate.title || tail(candidate.resource)}</DocumentLink></h2>
      <small className="mx-6 mt-1 block wrap-anywhere font-mono text-xs text-muted-foreground">{candidate.resource}</small>
      {text.error ? <ErrorBox error={text.error} /> : text.data ? <pre>{text.data.text}</pre> : <div className="flex items-center gap-2 py-6 text-sm text-muted-foreground"><Spinner /> Reading…</div>}
    </article>
    <aside className="grid gap-3 xl:sticky xl:top-5">
      {gate && <section className={`grid gap-3 rounded-lg border bg-card p-4 [&_h3]:flex [&_h3]:items-baseline [&_h3]:justify-between [&_h3]:gap-2 [&_h3]:text-sm [&_h3]:font-medium [&_small]:text-xs [&_small]:text-muted-foreground`}>
        <h3>{gate.id} <small>model: {gateTop || '—'} {gateTop ? (candidate.classifications[`${gate.id}:${gateTop}`] ?? 0).toFixed(2) : ''}</small></h3>
        <div className="grid grid-cols-2 gap-2 [&_button]:h-11">
          <Button variant="secondary" className="" disabled={busy} onClick={() => swipe(false)}><ThumbsDown />Junk <kbd>←</kbd></Button>
          <Button className="" disabled={busy} onClick={() => swipe(true)}><ThumbsUp />Record <kbd>→</kbd></Button>
        </div>
        <Segmented small label={gate.id} value={gateChoice} onChange={value => set(gate.id, value)} options={(gate.options || []).map(option => [option.id, option.id] as [string, string])} />
      </section>}
      {questions.filter(question => isChoice(question) && question.id !== gate?.id).map(question => <section className="grid gap-3 rounded-lg border bg-card p-4 [&_h3]:flex [&_h3]:items-baseline [&_h3]:justify-between [&_h3]:gap-2 [&_h3]:text-sm [&_h3]:font-medium [&_small]:text-xs [&_small]:text-muted-foreground" key={question.id}>
        <h3>{question.id} <small>model: {candidate.outcomes[question.id]?.top || 'unscored'}</small></h3>
        <Segmented small label={question.id} value={String(answers[question.id] ?? '')} onChange={value => set(question.id, value)} options={(question.options || []).map(option => [option.id, option.id] as [string, string])} />
      </section>)}
      <section className="grid gap-3 rounded-lg border bg-card p-4 [&_h3]:flex [&_h3]:items-baseline [&_h3]:justify-between [&_h3]:gap-2 [&_h3]:text-sm [&_h3]:font-medium [&_small]:text-xs [&_small]:text-muted-foreground">
        <h3>Tags <small>click to flip</small></h3>
        <div className="flex flex-wrap gap-2">
          {questions.filter(question => !isChoice(question)).map(question => {
            const on = answers[question.id] === true
            const score = candidate.classifications[question.id]
            const changed = edits[question.id] != null && edits[question.id] !== model[question.id]
            return <button type="button" key={question.id} className={`inline-flex items-center gap-1 rounded-full border px-3 py-1 text-xs hover:bg-accent [&_small]:text-muted-foreground ${on ? 'bg-primary text-primary-foreground' : ''} ${changed ? 'ring-2 ring-ring' : ''}`} aria-pressed={on} onClick={() => set(question.id, !on)}>
              {question.id}{score != null && <small>{score.toFixed(2)}</small>}
            </button>
          })}
        </div>
      </section>
      <section className="grid gap-3 rounded-lg border bg-card p-4 [&_h3]:flex [&_h3]:items-baseline [&_h3]:justify-between [&_h3]:gap-2 [&_h3]:text-sm [&_h3]:font-medium [&_small]:text-xs [&_small]:text-muted-foreground">
        <label className="grid min-w-0 gap-2 text-xs font-medium [&_[data-slot=select-trigger]]:w-full"><span>Propose tags</span><Input value={proposed} placeholder="ordinance amendment, fee schedule" onChange={event => setProposed(event.target.value)} /></label>
        <label className="grid min-w-0 gap-2 text-xs font-medium [&_[data-slot=select-trigger]]:w-full"><span>Note</span><Textarea rows={2} value={note} onChange={event => setNote(event.target.value)} /></label>
      </section>
      <div className="flex justify-between gap-2">
        <Button variant="ghost" disabled={busy} onClick={onSkip}>Skip <kbd>s</kbd></Button>
        <Button variant="secondary" disabled={busy} onClick={() => onSubmit(answers, proposed, note)}>{busy ? <Spinner /> : <Check />}Record as shown <kbd>↵</kbd></Button>
      </div>
      {error && <ErrorBox error={error} />}
      <p className="text-xs leading-relaxed text-muted-foreground"><Keyboard /> A verdict acts at once: a record said of an excluded page restores it, a tag flipped on is on the document for search.</p>
    </aside>
  </div>
}

/** How the model is doing against the people, from the same ledger the cards write. */
function Agreement({ evaluation }: { evaluation: Evaluation }) {
  const percent = (value?: number | null) => value == null ? '—' : `${Math.round(value * 100)}%`
  return <section className="rounded-lg border bg-card text-card-foreground shadow-sm mt-6">
    <div className="flex min-h-11 items-center justify-between border-b px-4 text-sm font-medium [&_small]:text-muted-foreground"><span>Agreement so far</span><small>{number(evaluation.reviews)} reviews · {number(evaluation.documents)} documents · `centinel evaluate` has the detail</small></div>
    <div className="overflow-x-auto"><Table className="[&_th:first-child]:w-[28%] [&_b]:block [&_b]:font-mono [&_b]:text-xs"><TableHeader><TableRow><TableHead>Question</TableHead><TableHead>Compared</TableHead><TableHead>Agree</TableHead><TableHead>Threshold</TableHead><TableHead>Would agree most at</TableHead><TableHead>Precision</TableHead><TableHead>Recall</TableHead></TableRow></TableHeader><TableBody>
      {evaluation.questions.map(question => <TableRow key={question.id}><TableCell><b>{question.id}</b><small>{question.kind}</small></TableCell><TableCell>{question.compared ? number(question.compared) : '—'}</TableCell><TableCell>{percent(question.agreement)}</TableCell><TableCell>{question.threshold.toFixed(2)}</TableCell><TableCell>{question.suggested_threshold == null ? '—' : question.suggested_threshold.toFixed(2)}</TableCell><TableCell>{percent(question.precision)}</TableCell><TableCell>{percent(question.recall)}</TableCell></TableRow>)}
    </TableBody></Table></div>
    {Object.keys(evaluation.proposed).length > 0 && <p className="border-t px-4 py-3 text-xs text-muted-foreground">Proposed tags: {Object.entries(evaluation.proposed).sort((a, b) => b[1] - a[1]).map(([name, count]) => `${name} ×${count}`).join(' · ')}</p>}
  </section>
}

/** Review before its queue arrives: the real header, the card and its panel masked. */
export function ReviewSkeleton() {
  return <div aria-busy>
    <PageHeader title="Review" detail="Read the document, say what it is. Right arrow for a record, left for junk, Enter to record the card as it stands." />
    <Skeleton className="mb-4 h-9 w-40" />
    <div className="grid items-start gap-4 xl:grid-cols-[minmax(0,1fr)_340px]">
      <div className="grid gap-4 rounded-lg border p-6">
        <Skeleton mask="Community Foundation Tampa Bay Speaker Series" className="text-2xl font-semibold" />
        <Skeleton mask="https://www.cftampabay.org/events/outstanding-speaker-series" className="font-mono text-xs" />
        {Array.from({ length: 6 }, (_, i) => <Skeleton key={i} className="h-4" />)}
      </div>
      <div className="grid gap-4"><Skeleton className="h-32 rounded-lg" /><Skeleton className="h-48 rounded-lg" /></div>
    </div>
  </div>
}
