import { useEffect, useMemo, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Check, ListPlus, Play, Plus, Trash2, X } from 'lucide-react'
import { Panel, ReactFlowProvider } from '@xyflow/react'
import { chainOf, QuestionFlow, sourcesOf, type TreeQuestion } from './tree'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { api, type ChoiceOption, type CorpusFilters, type Document, type Preset, type Question, type QuestionAction, type RunDetailQuery } from './api'
import { queries } from './queries'
import { Button } from '@/components/ui/button'
import { Skeleton } from '@/components/ui/skeleton'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { compact, money, number, plural, seconds, tail } from './format'
import { LiveRun, useRunDetail } from './live'
import { decisionLabels, decisionOf, estimateRun, isChoice, missingOther, outcomesOf, questionProblem, questionSnapshot, reachOf, tagsOf } from './policy'
import { ResultsSection } from './results'
import { priceOf } from './spend-logic'
import { ErrorBox, Empty, PageHeader, Segmented, Spinner } from './ui'

/** The TypeSafe published rate for Jev input, used when a run names no rate of its own. */
/** Must match `MAX_RUN_DOCUMENTS` in the workspace module. */
const MAX_RUN_DOCUMENTS = 50_000
const usageOptions: Array<[string, string]> = [['all', 'All usage'], ['included', 'Included'], ['excluded', 'Excluded'], ['pending', 'Pending']]
const actionOptions: Array<[QuestionAction, string]> = [['exclude', 'Exclude'], ['tag', 'Tag'], ['keep', 'Score only']]

type LocalQuestion = TreeQuestion
const newKey = () => globalThis.crypto?.randomUUID?.() || `${Date.now()}-${Math.random()}`
const withKeys = (questions: Question[], previous: LocalQuestion[] = []) => questions.map(question => {
  const before = previous.find(item => item.id === question.id)
  return { ...question, localKey: before?.localKey || newKey(), skip: before?.skip }
})
const stripKey = ({ localKey: _localKey, skip: _skip, ...question }: LocalQuestion): Question => question

/** Classifier filter choices: every question, and every option of every choice. */
export function classifierOptions(questions: Question[]): Array<[string, string]> {
  return questions.flatMap(question => isChoice(question)
    ? [[question.id, `${question.id} · junk probability`] as [string, string], ...(question.options || []).map(option => [`${question.id}:${option.id}`, `${question.id} · ${option.id}`] as [string, string])]
    : [[question.id, question.id] as [string, string]])
}

const blankNoul = (count: number): Question => ({ id: `question_${count + 1}`, kind: 'noul', instructions: '', version: 0, threshold: 0.9, review: 0.5, action: 'tag' })
const blankChoice = (count: number): Question => ({
  id: `choice_${count + 1}`, kind: 'choice', instructions: '', version: 0, threshold: 0.9, review: 0.5, action: 'tag',
  options: [{ id: 'first_kind', description: '', action: 'tag' }, { id: 'other', description: 'None of the other options fits.', action: 'keep' }],
})

export function Classify() {
  const client = useQueryClient()
  const saved = useQuery(queries.questions())
  // The shipped defaults come from the server, which is also what seeded the saved set,
  // so the page never carries a second copy of the questions.
  const shipped = useQuery(queries.presets())
  const [questions, setQuestions] = useState<LocalQuestion[]>([])
  const [savedQuestions, setSavedQuestions] = useState<Question[] | null>(null)
  const [loaded, setLoaded] = useState(false)
  const [scope, setScope] = useState<'all' | 'sample'>('sample')
  const [sample, setSample] = useState(25)
  const [model, setModel] = useState('jev-1.13.0')
  const [date, setDate] = useState(new Date().toISOString().slice(0, 10))
  const [filters, setFilters] = useState<CorpusFilters>({ search: '', address: '', source: '', usage: 'included', classifier: '', min_score: '0.5', max_score: '' })
  const [output, setOutput] = useState<'record' | 'preview'>('record')
  const [concurrency, setConcurrency] = useState(8)
  const [active, setActive] = useState<{ id: string; preview: boolean } | null>(null)

  useEffect(() => {
    if (!loaded && saved.data) {
      // The server seeds a new store with the defaults, so an empty saved set is one the
      // operator emptied on purpose; the Add menu offers the presets back.
      setQuestions(withKeys(saved.data.questions))
      setSavedQuestions(saved.data.questions)
      setLoaded(true)
    }
  }, [loaded, saved.data])

  const available = useQuery(queries.corpus(filters, 1, 1))
  const wire = questions.map(stripKey)
  const checked = questions.filter(question => !question.skip)
  const dirty = savedQuestions == null || questionSnapshot(wire) !== questionSnapshot(savedQuestions)
  const problems = questions.map(question => [question.id, questionProblem(question)] as const).filter(([, problem]) => problem)
  const savedById = new Map((savedQuestions || []).map(question => [question.id, question]))
  const isDirty = (question: LocalQuestion) => {
    const before = savedById.get(question.id)
    return !before || questionSnapshot([stripKey(question)]) !== questionSnapshot([before])
  }

  const applySaved = (response: { questions: Question[] }) => {
    setQuestions(current => withKeys(response.questions, current))
    setSavedQuestions(response.questions)
    client.setQueryData(['questions'], response)
  }
  const save = useMutation({ mutationFn: () => api.saveQuestions(wire), onSuccess: applySaved })

  const total = available.data?.total || 0
  const count = scope === 'all' ? Math.min(total, MAX_RUN_DOCUMENTS) : Math.min(sample, total)
  const recording = output === 'record'
  const prices = useQuery(queries.prices())
  const rate = priceOf(model, prices.data?.prices ?? [])?.input ?? null
  const reach = (question: Question) => reachOf(question, wire, available.data?.facets?.scores)
  const estimate = total ? estimateRun((available.data?.total_chars || 0) * count / total, count, checked.map(stripKey), rate ?? 0, undefined, reach) : null
  const run = useMutation({
    mutationFn: async () => {
      let asked = checked.map(stripKey)
      // A saved run needs saved questions. Saving here is one click fewer, and the
      // server still refuses anything it did not version.
      if (recording && dirty) {
        const response = await api.saveQuestions(wire)
        applySaved(response)
        const ids = new Set(asked.map(question => question.id))
        asked = response.questions.filter(question => ids.has(question.id))
      }
      return api.run({
        selection: {
          search: filters.search, address: filters.address, source: filters.source, usage: filters.usage, classifier: filters.classifier,
          ...(filters.classifier ? { min_score: Number(filters.min_score || 0) } : {}),
          ...(filters.classifier && filters.max_score ? { max_score: Number(filters.max_score) } : {}),
          count,
        },
        questions: asked, model, evaluation_date: date, record: recording,
        settings: { concurrency },
      })
    },
    onSuccess: response => setActive({ id: response.id, preview: !recording }),
  })
  // The same query the live panel polls, so the button knows when the run ends.
  const activeRun = useRunDetail(active?.id || '', { page: 1, page_size: 1 }, 700)
  const busy = Boolean(active) && (!activeRun.data || activeRun.data.status === 'running')

  const edit = (localKey: string, patch: Partial<LocalQuestion>) => setQuestions(current => current.map(question => question.localKey === localKey ? { ...question, ...patch } : question))
  const blocked = problems.length > 0 || !count || !checked.length || !model.trim() || run.isPending || Boolean(busy)
  const label = run.isPending ? 'Starting…' : `${recording ? (dirty ? 'Save and classify' : 'Classify') : 'Preview'} ${scope === 'all' ? 'all ' : ''}${plural(count, 'document')}`

  // Renaming a question keeps the follow-ups that hang off its answers.
  const editQuestion = (localKey: string, patch: Partial<LocalQuestion>) => setQuestions(current => {
    const before = current.find(question => question.localKey === localKey)
    const renamed = before && patch.id !== undefined && patch.id !== before.id ? before.id : null
    return current.map(question => {
      if (question.localKey === localKey) return { ...question, ...patch }
      if (renamed && question.when && (question.when === renamed || question.when.startsWith(`${renamed}:`))) return { ...question, when: `${patch.id}${question.when.slice(renamed.length)}` }
      return question
    })
  })
  const [editKey, setEditKey] = useState('')
  const [dialog, setDialog] = useState<'' | 'test' | 'run'>('')
  const [sourceKey, setSourceKey] = useState('')
  const sources = sourcesOf(questions)
  const source = sources.find(question => question.localKey === sourceKey) || sources[0]
  const chain = useMemo(() => source ? chainOf(source, questions) : [], [source, questions])
  const editing = questions.find(question => question.localKey === editKey)
  // Deleting a question keeps its chain: its follow-ups move up to the answer it followed.
  const remove = (localKey: string) => {
    const gone = questions.find(question => question.localKey === localKey)
    if (!gone) return
    const tags = new Set(outcomesOf(gone).flatMap(branch => branch.tag ? [branch.tag] : []))
    const next = questions.filter(question => question.localKey !== localKey).map(question => question.when && tags.has(question.when) ? { ...question, when: gone.when } : question)
    setQuestions(next)
    // A deleted source hands the chain to its first follow-up.
    if (source?.localKey === localKey) setSourceKey(questions.find(question => question.when && tags.has(question.when))?.localKey || '')
    if (editKey === localKey) setEditKey('')
  }
  const follow = (tag: string) => {
    const fresh = withKeys([{ ...blankNoul(questions.length), id: uniqueId(questions, `${tag.replace(':', '_')}_follow_up`), when: tag }])
    setQuestions(current => [...current, ...fresh])
    setEditKey(fresh[0].localKey)
  }
  const addSource = (added: Question[]) => {
    const fresh = withKeys(added.filter(question => !questions.some(existing => existing.id === question.id)))
    setQuestions(current => [...current, ...fresh])
    const root = fresh.find(question => !question.when)
    if (root) setSourceKey(root.localKey)
    if (fresh.length === 1) setEditKey(fresh[0].localKey)
  }
  const toggle = (localKey: string, runIt: boolean) => edit(localKey, { skip: !runIt })
  const test = useTest(checked.map(stripKey), model, date)
  const error = shipped.error || saved.error || save.error

  return <div className="relative -mx-5 -my-6 h-[calc(100svh-3rem)] overflow-hidden md:-mx-10 md:-my-8 md:h-[calc(100svh-1.25rem-2px)] md:rounded-2xl">
    <ReactFlowProvider>
      <QuestionFlow chain={chain} isDirty={isDirty} answers={test.answers} onEdit={setEditKey} onDelete={remove} onFollow={follow} onToggle={toggle}>
        <Panel position="top-left" className="!m-4 flex flex-wrap items-center gap-3 rounded-xl bg-background/95 px-4 py-2.5 shadow-[0_0_0_1px_var(--rule),0_4px_14px_rgba(26,23,18,0.06)]">
          <span className="font-serif text-[28px] leading-none">Classify</span>
          <span className="h-6 w-px bg-rule" />
          <span className="text-xs text-muted-foreground">Source</span>
          <Select value={source?.localKey || ''} onValueChange={setSourceKey}>
            <SelectTrigger className="h-8 w-80 text-[13px]"><SelectValue placeholder="No source yet" /></SelectTrigger>
            <SelectContent>{sources.map(question => <SelectItem key={question.localKey} value={question.localKey}><span className="font-mono">{question.id}</span><span className="text-muted-foreground"> · {plural(chainOf(question, questions).length, 'question')}</span></SelectItem>)}</SelectContent>
          </Select>
          <AddMenu onAdd={addSource} count={questions.length} existing={questions.map(question => question.id)} presets={shipped.data?.presets || []} />
        </Panel>
        <Panel position="top-right" className="!m-4 flex items-center gap-2">
          <button type="button" onClick={() => setDialog('test')} className="inline-flex h-9 max-w-80 items-center gap-2.5 rounded-full bg-foreground pr-3 pl-3 text-[13px] text-parchment shadow-[0_4px_14px_rgba(26,23,18,0.18)]">
            <span className={`size-2 shrink-0 rounded-full ${test.doc ? 'bg-flame shadow-[0_0_0_3px_rgba(200,118,30,0.35)]' : 'bg-muted-foreground'}`} />
            <span className="text-parchment/70">{test.running ? 'Asking Jev…' : test.doc ? 'Testing' : 'Test a document'}</span>
            {test.doc && <span className="truncate font-semibold">{test.doc.title || tail(test.doc.resource)}</span>}
          </button>
          <Button variant="outline" className="bg-background" onClick={() => setDialog('run')}>{busy ? <span className="size-2 rounded-full bg-flame" /> : <Play />}Run</Button>
          <Button disabled={save.isPending || !dirty || problems.length > 0} onClick={() => save.mutate()}>{save.isPending ? <Spinner /> : <Check />}{dirty ? 'Save tree' : 'Saved'}</Button>
        </Panel>
        {test.result && <Panel position="bottom-right" className="!m-4 grid w-72 gap-1.5 rounded-xl bg-background p-4 shadow-[0_0_0_1px_var(--rule),0_4px_14px_rgba(26,23,18,0.08)]">
          <span className="text-[10px] font-bold tracking-[0.1em] text-muted-foreground">OUTCOME FOR THIS DOCUMENT</span>
          <b className="text-[15px]">{test.result.error ? 'Jev could not answer' : `${decisionLabels[decisionOf(test.result)]}${tagsOf(test.result).length ? ` · ${tagsOf(test.result).join(', ')}` : ''}`}</b>
          <span className="text-xs text-muted-foreground">{test.result.error || `${seconds(test.result.duration_ms)} · a preview, nothing saved`}</span>
          <span className="mt-1 flex gap-2"><Button size="sm" variant="outline" disabled={test.running} onClick={test.again}>Ask again</Button><Button size="sm" variant="ghost" onClick={test.clear}>Clear</Button></span>
        </Panel>}
        <Panel position="bottom-center" className="!mb-4 grid justify-items-center gap-2">
          {error && <ErrorBox error={error} />}
          <span className="rounded-full bg-background/90 px-3 py-1 text-xs text-muted-foreground shadow-[0_0_0_1px_var(--rule)]">Drag to move · scroll to zoom · hover an answer and press + to ask a follow-up</span>
        </Panel>
      </QuestionFlow>
    </ReactFlowProvider>
    {!questions.length && saved.data && <div className="absolute inset-0 grid place-items-center"><div className="grid justify-items-center gap-3 rounded-xl bg-background p-8 text-center shadow-[0_0_0_1px_var(--rule)]"><span className="font-serif text-2xl">Start a chain</span><p className="max-w-72 text-sm text-muted-foreground">Add the junk gate or another preset as a source, then ask follow-ups of its answers.</p><AddMenu onAdd={addSource} count={0} existing={[]} presets={shipped.data?.presets || []} /></div></div>}

    <Dialog open={Boolean(editing)} onOpenChange={open => !open && setEditKey('')}>
      <DialogContent className="max-h-[88vh] overflow-y-auto sm:max-w-xl">
        {editing && <>
          <DialogHeader><DialogTitle className="font-serif text-2xl font-normal">{editing.id}</DialogTitle><DialogDescription>{editing.when ? `Asked only of documents tagged ${editing.when}.` : 'A source: asked of every document.'}</DialogDescription></DialogHeader>
          <QuestionEditor question={editing} questions={questions} edit={patch => editQuestion(editing.localKey, patch)} remove={() => remove(editing.localKey)} problem={questionProblem(editing)} />
        </>}
      </DialogContent>
    </Dialog>

    <Dialog open={dialog === 'test'} onOpenChange={open => !open && setDialog('')}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader><DialogTitle className="font-serif text-2xl font-normal">Test a document</DialogTitle><DialogDescription>Jev answers this chain for one document. Nothing is saved.</DialogDescription></DialogHeader>
        <TestPanel test={test} onPicked={() => setDialog('')} />
      </DialogContent>
    </Dialog>

    <Dialog open={dialog === 'run'} onOpenChange={open => !open && setDialog('')}>
      <DialogContent className="max-h-[88vh] overflow-y-auto sm:max-w-2xl">
        <DialogHeader><DialogTitle className="font-serif text-2xl font-normal">Run</DialogTitle><DialogDescription>Ask Jev the checked questions about a set of documents. A follow-up is asked only of the documents whose answer leads to it.</DialogDescription></DialogHeader>
        {active && <LiveRun key={active.id} id={active.id} preview={active.preview} onDismiss={() => setActive(null)} />}
        {active?.preview && <PreviewResults id={active.id} />}
        <div className="grid gap-3">
        <div className="grid min-w-0 gap-2 text-xs font-medium [&_[data-slot=select-trigger]]:w-full"><span>Documents</span><Segmented label="Run scope" value={scope} onChange={setScope} options={[['sample', 'A sample'], ['all', 'All matching']]} /></div>
          {scope === 'sample' && <label className="grid min-w-0 gap-2 text-xs font-medium [&_[data-slot=select-trigger]]:w-full"><span>How many</span><Input type="number" min="1" max={MAX_RUN_DOCUMENTS} value={sample} onChange={event => setSample(Math.min(MAX_RUN_DOCUMENTS, Math.max(1, Number(event.target.value) || 1)))} /></label>}
          <div className="grid grid-cols-2 gap-3">
            <label className="grid min-w-0 gap-2 text-xs font-medium [&_[data-slot=select-trigger]]:w-full"><span>Source</span><Select value={filters.source || 'all'} onValueChange={value => setFilters(current => ({ ...current, source: value === 'all' ? '' : value }))}><SelectTrigger><SelectValue /></SelectTrigger><SelectContent><SelectItem value="all">All sources</SelectItem>{(available.data?.sources || []).map(source => <SelectItem value={source} key={source}>{source}</SelectItem>)}</SelectContent></Select></label>
            <label className="grid min-w-0 gap-2 text-xs font-medium [&_[data-slot=select-trigger]]:w-full"><span>Usage</span><Select value={filters.usage} onValueChange={usage => setFilters(current => ({ ...current, usage }))}><SelectTrigger><SelectValue /></SelectTrigger><SelectContent>{usageOptions.map(([value, text]) => <SelectItem value={value} key={value}>{text}</SelectItem>)}</SelectContent></Select></label>
          </div>
          <details className="border-t pt-3 open:grid open:gap-3 [&_summary]:cursor-pointer [&_summary]:text-sm [&_summary]:font-medium">
            <summary>More filters</summary>
            <label className="grid min-w-0 gap-2 text-xs font-medium [&_[data-slot=select-trigger]]:w-full"><span>Text contains</span><Input value={filters.search} onChange={event => setFilters(current => ({ ...current, search: event.target.value }))} placeholder="Words in the document" /></label>
            <label className="grid min-w-0 gap-2 text-xs font-medium [&_[data-slot=select-trigger]]:w-full"><span>Address contains</span><Input value={filters.address} onChange={event => setFilters(current => ({ ...current, address: event.target.value }))} placeholder="agenda.pdf" /></label>
            <label className="grid min-w-0 gap-2 text-xs font-medium [&_[data-slot=select-trigger]]:w-full"><span>Only where a score is</span><Select value={filters.classifier || 'all'} onValueChange={value => setFilters(current => ({ ...current, classifier: value === 'all' ? '' : value }))}><SelectTrigger><SelectValue /></SelectTrigger><SelectContent><SelectItem value="all">Any score</SelectItem>{classifierOptions(savedQuestions || []).map(([value, text]) => <SelectItem value={value} key={value}>{text}</SelectItem>)}</SelectContent></Select></label>
            {filters.classifier && <div className="grid grid-cols-2 gap-3"><label className="grid min-w-0 gap-2 text-xs font-medium [&_[data-slot=select-trigger]]:w-full"><span>From</span><Input type="number" min="0" max="1" step="0.05" value={filters.min_score} onChange={event => setFilters(current => ({ ...current, min_score: event.target.value }))} /></label><label className="grid min-w-0 gap-2 text-xs font-medium [&_[data-slot=select-trigger]]:w-full"><span>To</span><Input type="number" min="0" max="1" step="0.05" placeholder="1" value={filters.max_score} onChange={event => setFilters(current => ({ ...current, max_score: event.target.value }))} /></label></div>}
          </details>
          <details className="border-t pt-3 open:grid open:gap-3 [&_summary]:cursor-pointer [&_summary]:text-sm [&_summary]:font-medium">
            <summary>Model, output, and cost</summary>
            <div className="grid min-w-0 gap-2 text-xs font-medium [&_[data-slot=select-trigger]]:w-full"><span>Output</span><Segmented label="Run output" value={output} onChange={setOutput} options={[['record', 'Save a run'], ['preview', 'Preview only']]} /></div>
            <div className="grid grid-cols-2 gap-3">
              <label className="grid min-w-0 gap-2 text-xs font-medium [&_[data-slot=select-trigger]]:w-full"><span>Model</span><Input value={model} onChange={event => setModel(event.target.value)} /></label>
              <label className="grid min-w-0 gap-2 text-xs font-medium [&_[data-slot=select-trigger]]:w-full"><span>In flight</span><Input type="number" min="1" max="32" value={concurrency} onChange={event => setConcurrency(Math.min(32, Math.max(1, Number(event.target.value) || 1)))} /></label>
            </div>
            <div className="grid grid-cols-2 gap-3">
              <label className="grid min-w-0 gap-2 text-xs font-medium [&_[data-slot=select-trigger]]:w-full"><span>Evaluation date</span><Input type="date" value={date} onChange={event => setDate(event.target.value)} /></label>
            </div>
          </details>

          <div className="grid grid-cols-[auto_1fr] items-baseline gap-x-2 rounded-md border bg-muted p-3 [&_b]:text-2xl [&_span]:text-xs [&_small]:col-span-full [&_small]:text-xs [&_small]:text-muted-foreground">
            <b>{number(count)}</b><span>{count === 1 ? 'document' : 'documents'} × {plural(checked.length, 'question')}</span>
            {estimate && <small>About {compact(estimate.tokens)} input tokens{rate != null ? ` · ${money(estimate.cost)}` : ' · no price for this model'}</small>}
            <small>{number(total)} match the filters{total > MAX_RUN_DOCUMENTS ? `; one run holds ${number(MAX_RUN_DOCUMENTS)}` : ''}.</small>
          </div>
          {problems.length > 0 && <p className="rounded-md border bg-muted p-3 text-xs">Fix {problems[0][0]}: {problems[0][1]}</p>}
          {!checked.length && <p className="rounded-md border bg-muted p-3 text-xs">Check at least one question.</p>}
          <Button className="w-full h-11" disabled={blocked} onClick={() => run.mutate()}>{run.isPending ? <Spinner /> : <Play />}{label}</Button>
          {busy && <p className="text-xs leading-relaxed text-muted-foreground">A run is going. It shows above.</p>}
          {available.error && <ErrorBox error={available.error} />}{run.error && <ErrorBox error={run.error} />}
          <p className="text-xs leading-relaxed text-muted-foreground">{recording ? 'A saved run does not change what search returns until you commit it.' : 'A preview is not saved and cannot be committed.'}</p>
        </div>
      </DialogContent>
    </Dialog>
  </div>
}

function PreviewResults({ id }: { id: string }) {
  const [view, setView] = useState<RunDetailQuery>({ page: 1, page_size: 50, outcome: '', sort: 'decision', direction: '' })
  const detail = useRunDetail(id, view)
  if (!detail.data || detail.data.status === 'running') return null
  const questions = detail.data.effective_questions?.length ? detail.data.effective_questions : detail.data.questions
  return <section className="rounded-lg border bg-card text-card-foreground shadow-sm mb-4 p-4"><ResultsSection run={detail.data} questions={questions} view={view} setView={setView} loading={detail.isFetching} /></section>
}

function AddMenu({ onAdd, count, existing, presets }: { onAdd: (questions: Question[]) => void; count: number; existing: string[]; presets: Preset[] }) {
  const [open, setOpen] = useState(false)
  const pick = (questions: Question[]) => { onAdd(questions); setOpen(false) }
  return <div className="relative" onBlur={event => { if (!event.currentTarget.contains(event.relatedTarget as Node)) setOpen(false) }}>
    <Button size="sm" onClick={() => setOpen(value => !value)} aria-expanded={open}><ListPlus />Add</Button>
    {open && <div className="absolute right-0 top-full z-30 mt-2 w-80 rounded-lg border bg-popover p-1 shadow-md [&_button]:grid [&_button]:w-full [&_button]:gap-1 [&_button]:rounded [&_button]:p-2 [&_button]:text-left [&_button]:hover:bg-accent [&_button]:disabled:opacity-50 [&_small]:text-xs [&_small]:text-muted-foreground [&_hr]:my-1" role="menu">
      {presets.map(preset => {
        const added = preset.questions.every(question => existing.includes(question.id))
        return <button type="button" role="menuitem" key={preset.id} disabled={added} onClick={() => pick(preset.questions)}>
          <b>{preset.label}{added ? ' · added' : ''}</b><small>{preset.detail}</small>
        </button>
      })}
      <hr />
      <button type="button" role="menuitem" onClick={() => pick([blankNoul(count)])}><b>Blank yes / no question</b><small>One probability. Good for a topic that a document can have or not.</small></button>
      <button type="button" role="menuitem" onClick={() => pick([blankChoice(count)])}><b>Blank choice</b><small>One option out of a set. Good for “what kind of document is this”.</small></button>
    </div>}
  </div>
}

function QuestionEditor({ question, questions, edit, remove, problem }: { question: LocalQuestion; questions: LocalQuestion[]; edit: (patch: Partial<LocalQuestion>) => void; remove: () => void; problem: string }) {
  const choice = isChoice(question)
  const options = question.options || []
  const setOption = (index: number, patch: Partial<ChoiceOption>) => edit({ options: options.map((option, at) => at === index ? { ...option, ...patch } : option) })
  const setKind = (kind: 'noul' | 'choice') => edit(kind === 'choice'
    ? { kind, options: options.length ? options : blankChoice(0).options }
    : { kind, options: [] })
  // A question may follow any answer but its own, and none of its own follow-ups'.
  const below = descendants(question, questions)
  const parents = questions.filter(other => other.localKey !== question.localKey && !below.has(other.localKey)).flatMap(other => outcomesOf(other).flatMap(outcome => outcome.tag ? [[outcome.tag, `${other.id} → ${outcome.label}`] as [string, string]] : []))
  return <div className="grid gap-4">
    <label className="grid min-w-0 gap-2 text-xs font-medium [&_[data-slot=select-trigger]]:w-full"><span>Asked of</span><Select value={question.when || 'root'} onValueChange={value => edit({ when: value === 'root' ? undefined : value })}><SelectTrigger><SelectValue /></SelectTrigger><SelectContent><SelectItem value="root">Every document</SelectItem>{parents.map(([tag, label]) => <SelectItem value={tag} key={tag}>Only {label}</SelectItem>)}</SelectContent></Select></label>
    <div className="grid items-end gap-4">
      <label className="grid min-w-0 gap-2 text-xs font-medium [&_[data-slot=select-trigger]]:w-full"><span>Id</span><Input value={question.id} onChange={event => edit({ id: event.target.value })} /></label>
      <div className="grid min-w-0 gap-2 text-xs font-medium [&_[data-slot=select-trigger]]:w-full"><span>Kind</span><Segmented label="Question kind" value={question.kind || 'noul'} onChange={setKind} options={[['noul', 'Yes / no'], ['choice', 'Choice']]} /></div>
      {!choice && <div className="grid min-w-0 gap-2 text-xs font-medium [&_[data-slot=select-trigger]]:w-full"><span>When yes</span><Segmented label="Action" value={question.action} onChange={action => edit({ action })} options={actionOptions} /></div>}
    </div>
    <label className="grid min-w-0 gap-2 text-xs font-medium [&_[data-slot=select-trigger]]:w-full"><span>{choice ? 'Question' : 'Question (answered yes or no)'}</span><Textarea rows={2} value={question.instructions} placeholder={choice ? 'What is this text mainly?' : 'Does `text` …?'} onChange={event => edit({ instructions: event.target.value })} /></label>
    {choice && <div className="grid min-w-0 gap-2 text-xs font-medium [&_[data-slot=select-trigger]]:w-full"><span>Options</span>
      <div className="grid gap-3">
        {options.map((option, index) => <div className="grid grid-cols-[minmax(0,1fr)_auto] items-start gap-2 [&_[role=radiogroup]]:col-span-full [&_textarea]:col-span-full" key={index}>
          <Input aria-label="Option id" value={option.id} onChange={event => setOption(index, { id: event.target.value })} />
          <Segmented small label={`${option.id} action`} value={option.action} onChange={action => setOption(index, { action })} options={actionOptions} />
          <Button aria-label={`Remove option ${option.id}`} variant="ghost" size="icon" onClick={() => edit({ options: options.filter((_, at) => at !== index) })}><X /></Button>
          <Textarea rows={2} aria-label={`${option.id} description`} value={option.description} placeholder="What it is, the words that show it, and what it is not." onChange={event => setOption(index, { description: event.target.value })} />
        </div>)}
        <Button variant="ghost" size="sm" onClick={() => edit({ options: [...options, { id: `option_${options.length + 1}`, description: '', action: 'keep' }] })}><Plus />Add option</Button>
        {missingOther(question) && <p className="text-xs leading-relaxed text-muted-foreground">Add an <code>other</code> option. Without it, a document that fits nothing is forced into a wrong option.</p>}
      </div>
    </div>}
    <PolicyControl question={question} edit={edit} />
    <div className="flex items-center justify-between gap-3">
      {problem ? <p className="text-xs leading-relaxed text-muted-foreground text-destructive">{problem}</p> : <span />}
      <Button variant="ghost" size="sm" onClick={remove}><Trash2 />Remove question</Button>
    </div>
  </div>
}

/** Threshold and review floor on one 0–1 scale, so the three zones read at a glance. */
function PolicyControl({ question, edit }: { question: Question; edit: (patch: Partial<Question>) => void }) {
  const threshold = question.threshold
  const review = question.review
  const pct = (value: number) => `${value * 100}%`
  const choice = isChoice(question)
  return <div className="grid grid-cols-1 gap-3 rounded-md border bg-muted p-3 [&>p]:col-span-full [&_label]:grid [&_label]:gap-1 [&_label]:text-xs [&_input]:w-full [&_input]:accent-flame">
    <div className="col-span-full flex h-6 overflow-hidden rounded text-xs [&_i]:overflow-hidden [&_i]:px-2 [&_i]:not-italic" aria-hidden>
      <i className="bg-muted text-muted-foreground" style={{ width: pct(review ?? threshold) }}>{(review ?? threshold) > 0.2 ? 'no action' : ''}</i>
      {review != null && <i className="bg-secondary" style={{ width: pct(threshold - review) }}>{threshold - review > 0.15 ? 'review' : ''}</i>}
      <i className="bg-primary text-primary-foreground" style={{ width: pct(1 - threshold) }}>{1 - threshold > 0.08 ? 'act' : ''}</i>
    </div>
    <label><span>Act at <b>{threshold.toFixed(2)}</b></span><input type="range" min=".5" max="1" step=".01" value={threshold} onChange={event => { const next = Number(event.target.value); edit({ threshold: next, review: review != null ? Math.min(review, next) : review }) }} /></label>
    <label><span>{review != null ? <>Review from <b>{review.toFixed(2)}</b></> : 'No review band'}</span><input type="range" min="0" max={threshold} step=".01" value={review ?? threshold} onChange={event => { const next = Number(event.target.value); edit({ review: next >= threshold ? null : next }) }} /></label>
    <p className="text-xs leading-relaxed text-muted-foreground">{choice ? 'For a choice, the exclude options are added together and compared with “act at”. A tag option acts when it alone reaches it.' : 'A score at or above “act at” applies the action. A score in the review band is held for a person.'}</p>
  </div>
}

function descendants(question: LocalQuestion, questions: LocalQuestion[]): Set<string> {
  const found = new Set<string>()
  const walk = (from: Question) => outcomesOf(from).forEach(branch => questions.filter(q => branch.tag && q.when === branch.tag && !found.has(q.localKey)).forEach(child => { found.add(child.localKey); walk(child) }))
  walk(question)
  return found
}

function uniqueId(questions: Question[], base: string) {
  const ids = new Set(questions.map(question => question.id))
  if (!ids.has(base)) return base
  let n = 2
  while (ids.has(`${base}_${n}`)) n++
  return `${base}_${n}`
}

type Test = ReturnType<typeof useTest>

/**
 * One document through the tree as a preview run: nothing is saved, and draft questions
 * are allowed. The server follows the chain the way a run does, so Jev is asked only the
 * checked questions the document reaches; the rest come back unanswered and stay dimmed.
 */
function useTest(questions: Question[], model: string, date: string) {
  const [doc, setDoc] = useState<Document | null>(null)
  const start = useMutation({
    mutationFn: (picked: Document) => api.run({ documents: [{ source: picked.source, resource: picked.resource, derived_sha: picked.derived_sha }], questions, model, evaluation_date: date, record: false, settings: { concurrency: 1 } }),
  })
  const detail = useRunDetail(start.data?.id || '', { page: 1, page_size: 1 }, 700)
  const result = detail.data?.status !== 'running' ? detail.data?.results?.[0] : undefined
  const pick = (picked: Document) => { setDoc(picked); start.mutate(picked) }
  const clear = () => { setDoc(null); start.reset() }
  return { doc, pick, clear, result, answers: result && !result.error ? result.answers : undefined, running: start.isPending || detail.data?.status === 'running', error: start.error || detail.error, again: () => doc && start.mutate(doc) }
}

function TestPanel({ test, onPicked }: { test: Test; onPicked: () => void }) {
  const [text, setText] = useState('')
  const found = useQuery(queries.corpus({ address: text.trim() }, 1, 8))
  return <div className="grid gap-3">
    <Input value={text} onChange={event => setText(event.target.value)} placeholder="Find a document by address or title" />
    <div className="grid">
      {(found.data?.documents || []).map(doc => <button type="button" key={`${doc.source}:${doc.resource}`} onClick={() => { test.pick(doc); onPicked() }} className={`grid gap-0.5 border-b py-2 text-left hover:bg-hover ${test.doc?.resource === doc.resource ? 'font-semibold' : ''}`}>
        <span className="truncate text-sm">{doc.title || tail(doc.resource)}</span><span className="truncate text-xs text-muted-foreground">{doc.source} · {doc.resource}</span>
      </button>)}
    </div>
    {test.error && <ErrorBox error={test.error} />}
    <p className="text-xs leading-relaxed text-muted-foreground">Jev is asked only the questions on the lit path. The dimmed ones are not asked of this document, and cost nothing.</p>
  </div>
}

/** The canvas before the questions arrive: the same full-bleed frame, a source card masked. */
export function ClassifySkeleton() {
  return <div aria-busy className="relative -mx-5 -my-6 grid h-[calc(100svh-3rem)] place-items-center overflow-hidden bg-canvas bg-[radial-gradient(var(--dot)_1.4px,transparent_1.4px)] [background-size:18px_18px] md:-mx-10 md:-my-8 md:h-[calc(100svh-1.25rem-2px)] md:rounded-2xl">
    <span className="absolute top-4 left-4 rounded-xl bg-background/95 px-4 py-2.5 font-serif text-[28px] leading-none shadow-[0_0_0_1px_var(--rule)]">Classify</span>
    <div className="grid w-80 gap-3 rounded-xl bg-background p-4 shadow-[0_0_0_1.5px_var(--rule)]">
      <Skeleton mask="SOURCE · NOUL · v1" className="text-[10px]" />
      <Skeleton mask="Is this page unusable because it is empty or garbled?" className="text-[15px] leading-[21px]" />
      <div className="grid grid-cols-2 gap-1.5 border-t pt-3"><Skeleton className="h-[34px]" /><Skeleton className="h-[34px]" /></div>
    </div>
  </div>
}
