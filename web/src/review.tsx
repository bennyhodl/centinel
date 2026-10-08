import { useEffect, useMemo, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Check, Keyboard, ThumbsDown, ThumbsUp } from 'lucide-react'
import { api, type Evaluation, type Question, type ReviewCandidate } from './api'
import { Button } from '@/components/ui/button'
import { Input, Textarea } from '@/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { characters, number, tail } from './format'
import { isChoice } from './policy'
import { type Answers, buildReview, gateOf, gateOption, modelAnswers } from './review-logic'
import { DocumentLink, Empty, ErrorBox, PageHeader, Segmented, Spinner } from './ui'

const QUEUE = 20

/**
 * One document at a time. The text on the left, the policy's decisions on the right, and
 * the keyboard does the work: → record, ← junk, Enter records the card as it stands, s
 * skips. Every card is one line in the reviews ledger and acts on search at once.
 */
export function Review() {
  const client = useQueryClient()
  const questions = useQuery({ queryKey: ['questions'], queryFn: api.questions })
  const [source, setSource] = useState('')
  const [includeReviewed, setIncludeReviewed] = useState(false)
  const queue = useQuery({
    queryKey: ['review-queue', source, includeReviewed],
    queryFn: () => api.reviewQueue({ source, page_size: QUEUE, include_reviewed: includeReviewed }),
    staleTime: Infinity,
  })
  const sources = useQuery({ queryKey: ['review-sources'], queryFn: () => api.corpus(new URLSearchParams({ page: '1', page_size: '1' })), staleTime: 60_000 })
  const evaluation = useQuery({ queryKey: ['evaluation'], queryFn: api.evaluation })
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
    <PageHeader eyebrow="Human in the loop" title="Review" detail="Read the document, say what it is. Right arrow for a record, left for junk, Enter to record the card as it stands.">
      <div className="header-stat"><span>In the review band</span><b>{queue.data ? number(queue.data.in_review_band) : '—'}</b></div>
      <div className="header-stat"><span>Reviewed</span><b>{queue.data ? number(queue.data.reviewed) : '—'}</b></div>
    </PageHeader>
    <div className="review-toolbar">
      <label className="filter-field compact"><span>Source</span><Select value={source || 'all'} onValueChange={value => { setSource(value === 'all' ? '' : value); setCursor(0) }}><SelectTrigger aria-label="Source"><SelectValue /></SelectTrigger><SelectContent><SelectItem value="all">All sources</SelectItem>{(sources.data?.sources || []).map(value => <SelectItem value={value} key={value}>{value}</SelectItem>)}</SelectContent></Select></label>
      <label className="check"><input type="checkbox" checked={includeReviewed} onChange={event => { setIncludeReviewed(event.target.checked); setCursor(0) }} /> Show reviewed documents too</label>
      <span className="muted">{queue.data ? `${number(queue.data.scored)} scored` : ''}{last ? ` · ${last}` : ''}</span>
    </div>
    {queue.error ? <ErrorBox error={queue.error} /> : questions.error ? <ErrorBox error={questions.error} /> : !queue.data || !questions.data ? <div className="loading"><Spinner /> Reading the queue…</div>
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
  const text = useQuery({ queryKey: ['read', candidate.derived_sha, candidate.source, candidate.resource], queryFn: () => api.read({ source: candidate.source, resource: candidate.resource, derived_sha: candidate.derived_sha }) })
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
  return <div className="review-grid">
    <article className="document review-doc">
      <div className="document-meta"><span>{candidate.source}</span><span>{characters(candidate.chars)}</span><span>{candidate.excluded ? 'excluded now' : 'included now'}</span>{candidate.review_band && <span className="chip warn">review band</span>}{candidate.reviewed && <span className="chip">reviewed before</span>}<span className="grow" /><span>{position}</span></div>
      <h2 className="review-title"><DocumentLink doc={candidate}>{candidate.title || tail(candidate.resource)}</DocumentLink></h2>
      <small className="review-url">{candidate.resource}</small>
      {text.error ? <ErrorBox error={text.error} /> : text.data ? <pre>{text.data.text}</pre> : <div className="loading"><Spinner /> Reading…</div>}
    </article>
    <aside className="review-panel">
      {gate && <section className={`verdict gate ${junk ? 'junk' : 'keep'}`}>
        <h3>{gate.id} <small>model: {gateTop || '—'} {gateTop ? (candidate.classifications[`${gate.id}:${gateTop}`] ?? 0).toFixed(2) : ''}</small></h3>
        <div className="swipe">
          <Button variant="secondary" className="swipe-junk" disabled={busy} onClick={() => swipe(false)}><ThumbsDown />Junk <kbd>←</kbd></Button>
          <Button className="swipe-keep" disabled={busy} onClick={() => swipe(true)}><ThumbsUp />Record <kbd>→</kbd></Button>
        </div>
        <Segmented small label={gate.id} value={gateChoice} onChange={value => set(gate.id, value)} options={(gate.options || []).map(option => [option.id, option.id] as [string, string])} />
      </section>}
      {questions.filter(question => isChoice(question) && question.id !== gate?.id).map(question => <section className="verdict" key={question.id}>
        <h3>{question.id} <small>model: {candidate.outcomes[question.id]?.top || 'unscored'}</small></h3>
        <Segmented small label={question.id} value={String(answers[question.id] ?? '')} onChange={value => set(question.id, value)} options={(question.options || []).map(option => [option.id, option.id] as [string, string])} />
      </section>)}
      <section className="verdict">
        <h3>Tags <small>click to flip</small></h3>
        <div className="chips">
          {questions.filter(question => !isChoice(question)).map(question => {
            const on = answers[question.id] === true
            const score = candidate.classifications[question.id]
            const changed = edits[question.id] != null && edits[question.id] !== model[question.id]
            return <button type="button" key={question.id} className={`chip-toggle ${on ? 'on' : ''} ${changed ? 'changed' : ''}`} aria-pressed={on} onClick={() => set(question.id, !on)}>
              {question.id}{score != null && <small>{score.toFixed(2)}</small>}
            </button>
          })}
        </div>
      </section>
      <section className="verdict">
        <label className="field"><span>Propose tags</span><Input value={proposed} placeholder="ordinance amendment, fee schedule" onChange={event => setProposed(event.target.value)} /></label>
        <label className="field"><span>Note</span><Textarea rows={2} value={note} onChange={event => setNote(event.target.value)} /></label>
      </section>
      <div className="review-actions">
        <Button variant="ghost" disabled={busy} onClick={onSkip}>Skip <kbd>s</kbd></Button>
        <Button variant="secondary" disabled={busy} onClick={() => onSubmit(answers, proposed, note)}>{busy ? <Spinner /> : <Check />}Record as shown <kbd>↵</kbd></Button>
      </div>
      {error && <ErrorBox error={error} />}
      <p className="hint"><Keyboard /> A verdict acts at once: a record said of an excluded page restores it, a tag flipped on is on the document for search.</p>
    </aside>
  </div>
}

/** How the model is doing against the people, from the same ledger the cards write. */
function Agreement({ evaluation }: { evaluation: Evaluation }) {
  const percent = (value?: number | null) => value == null ? '—' : `${Math.round(value * 100)}%`
  return <section className="panel agreement">
    <div className="panel-top"><span>Agreement so far</span><small>{number(evaluation.reviews)} reviews · {number(evaluation.documents)} documents · `centinel evaluate` has the detail</small></div>
    <div className="table-wrap"><table className="eval-table"><thead><tr><th>Question</th><th>Compared</th><th>Agree</th><th>Threshold</th><th>Would agree most at</th><th>Precision</th><th>Recall</th></tr></thead><tbody>
      {evaluation.questions.map(question => <tr key={question.id}><td><b>{question.id}</b><small>{question.kind}</small></td><td>{question.compared ? number(question.compared) : '—'}</td><td>{percent(question.agreement)}</td><td>{question.threshold.toFixed(2)}</td><td>{question.suggested_threshold == null ? '—' : question.suggested_threshold.toFixed(2)}</td><td>{percent(question.precision)}</td><td>{percent(question.recall)}</td></tr>)}
    </tbody></table></div>
    {Object.keys(evaluation.proposed).length > 0 && <p className="proposed">Proposed tags: {Object.entries(evaluation.proposed).sort((a, b) => b[1] - a[1]).map(([name, count]) => `${name} ×${count}`).join(' · ')}</p>}
  </section>
}
