import { useEffect, useRef, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Check, ChevronDown, ListPlus, Play, Plus, Trash2, X } from 'lucide-react'
import { api, corpusParams, type ChoiceOption, type CorpusFilters, type Preset, type Question, type QuestionAction, type RunDetailQuery } from './api'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { compact, money, number, plural } from './format'
import { LiveRun, useRunDetail } from './live'
import { estimateRun, isChoice, missingOther, policyShort, questionProblem, questionSnapshot } from './policy'
import { ResultsSection } from './results'
import { ErrorBox, Empty, PageHeader, Segmented, Spinner } from './ui'

/** The TypeSafe published rate for Jev input, used when a run names no rate of its own. */
const JEV_INPUT_RATE = 0.042
/** Must match `MAX_RUN_DOCUMENTS` in the workspace module. */
const MAX_RUN_DOCUMENTS = 50_000
const usageOptions: Array<[string, string]> = [['all', 'All usage'], ['included', 'Included'], ['excluded', 'Excluded'], ['pending', 'Pending']]
const actionOptions: Array<[QuestionAction, string]> = [['exclude', 'Exclude'], ['tag', 'Tag'], ['keep', 'Score only']]

/** A question as edited here: a stable key for React, and whether the next run uses it. */
type LocalQuestion = Question & { localKey: string; skip?: boolean }
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
  const saved = useQuery({ queryKey: ['questions'], queryFn: api.questions })
  // The shipped defaults come from the server, which is also what seeded the saved set,
  // so the page never carries a second copy of the questions.
  const shipped = useQuery({ queryKey: ['presets'], queryFn: api.presets, staleTime: Infinity })
  const [questions, setQuestions] = useState<LocalQuestion[]>([])
  const [savedQuestions, setSavedQuestions] = useState<Question[] | null>(null)
  const [openKey, setOpenKey] = useState('')
  const [loaded, setLoaded] = useState(false)
  const [scope, setScope] = useState<'all' | 'sample'>('sample')
  const [sample, setSample] = useState(25)
  const [model, setModel] = useState('jev-1.13.0')
  const [date, setDate] = useState(new Date().toISOString().slice(0, 10))
  const [inputRate, setInputRate] = useState('')
  const [filters, setFilters] = useState<CorpusFilters>({ search: '', address: '', source: '', usage: 'included', classifier: '', min_score: '0.5', max_score: '' })
  const [output, setOutput] = useState<'record' | 'preview'>('record')
  const [concurrency, setConcurrency] = useState(8)
  const [active, setActive] = useState<{ id: string; preview: boolean } | null>(null)
  const top = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!loaded && saved.data) {
      // The server seeds a new store with the defaults, so an empty saved set is one the
      // operator emptied on purpose; the Add menu offers the presets back.
      setQuestions(withKeys(saved.data.questions))
      setSavedQuestions(saved.data.questions)
      setLoaded(true)
    }
  }, [loaded, saved.data])

  const countParams = corpusParams(filters, 1, 1)
  const available = useQuery({ queryKey: ['selection-count', countParams.toString()], queryFn: () => api.corpus(countParams) })
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
  const rate = inputRate.trim() ? Number(inputRate) : model.startsWith('jev-') ? JEV_INPUT_RATE : null
  const estimate = total ? estimateRun((available.data?.total_chars || 0) * count / total, count, checked.map(stripKey), rate ?? 0) : null
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
        settings: { concurrency, ...(inputRate.trim() ? { input_cost_per_million: Number(inputRate) } : {}) },
      })
    },
    onSuccess: response => {
      setActive({ id: response.id, preview: !recording })
      top.current?.scrollIntoView({ behavior: 'smooth', block: 'start' })
    },
  })
  // The same query the live panel polls, so the button knows when the run ends.
  const activeRun = useRunDetail(active?.id || '', { page: 1, page_size: 1 }, 700)
  const busy = Boolean(active) && (!activeRun.data || activeRun.data.status === 'running')

  const edit = (localKey: string, patch: Partial<LocalQuestion>) => setQuestions(current => current.map(question => question.localKey === localKey ? { ...question, ...patch } : question))
  const add = (added: Question[]) => {
    const fresh = withKeys(added.filter(question => !questions.some(existing => existing.id === question.id)))
    setQuestions(current => [...current, ...fresh])
    if (fresh.length === 1) setOpenKey(fresh[0].localKey)
  }
  const blocked = problems.length > 0 || !count || !checked.length || !model.trim() || run.isPending || Boolean(busy)
  const label = run.isPending ? 'Starting…' : `${recording ? (dirty ? 'Save and classify' : 'Classify') : 'Preview'} ${scope === 'all' ? 'all ' : ''}${plural(count, 'document')}`

  return <>
    <div ref={top} />
    <PageHeader eyebrow="Jev classifiers" title="Classify" detail="Choose the questions, choose the documents, and run. Every answer shows here as it comes back." />
    {active && <LiveRun key={active.id} id={active.id} preview={active.preview} onDismiss={() => setActive(null)} />}
    {active?.preview && <PreviewResults id={active.id} />}

    <div className="grid items-start gap-4 xl:grid-cols-[minmax(0,1fr)_320px]">
      <section className="rounded-lg border bg-card text-card-foreground shadow-sm py-2">
        <div className="flex items-center gap-2 border-b p-4 [&>div:first-child]:flex-1 [&_h2]:text-lg [&_h2]:font-semibold [&_p]:text-xs [&_p]:text-muted-foreground">
          <div><h2>Questions</h2><p>{number(checked.length)} of {plural(questions.length, 'question')} will run. Click a question to edit it.</p></div>
          <AddMenu onAdd={add} count={questions.length} existing={questions.map(question => question.id)} presets={shipped.data?.presets || []} />
          <Button variant="secondary" size="sm" disabled={save.isPending || !dirty || problems.length > 0} onClick={() => save.mutate()}>{save.isPending ? <Spinner /> : <Check />}{dirty ? 'Save' : 'Saved'}</Button>
        </div>
        {questions.map(question => <QuestionRow
          key={question.localKey}
          question={question}
          open={openKey === question.localKey}
          dirty={isDirty(question)}
          onToggle={() => setOpenKey(current => current === question.localKey ? '' : question.localKey)}
          edit={patch => edit(question.localKey, patch)}
          remove={() => setQuestions(current => current.filter(item => item.localKey !== question.localKey))}
        />)}
        {!questions.length && <Empty>Add the junk gate or another preset to start.</Empty>}
        {shipped.error && <ErrorBox error={shipped.error} />}
        {saved.error && <ErrorBox error={saved.error} />}{save.error && <ErrorBox error={save.error} />}
      </section>

      <aside className="grid gap-3 rounded-lg border bg-card p-4 xl:sticky xl:top-5 [&_h2]:font-semibold">
        <h2>Run</h2>
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
            <label className="grid min-w-0 gap-2 text-xs font-medium [&_[data-slot=select-trigger]]:w-full"><span>$ per 1M input</span><Input type="number" min="0" step="0.001" value={inputRate} placeholder={model.startsWith('jev-') ? String(JEV_INPUT_RATE) : 'unknown'} onChange={event => setInputRate(event.target.value)} /></label>
          </div>
        </details>

        <div className="grid grid-cols-[auto_1fr] items-baseline gap-x-2 rounded-md border bg-muted p-3 [&_b]:text-2xl [&_span]:text-xs [&_small]:col-span-full [&_small]:text-xs [&_small]:text-muted-foreground">
          <b>{number(count)}</b><span>{count === 1 ? 'document' : 'documents'} × {plural(checked.length, 'question')}</span>
          {estimate && <small>About {compact(estimate.tokens)} input tokens{rate != null ? ` · ${money(estimate.cost)}` : ''}</small>}
          <small>{number(total)} match the filters{total > MAX_RUN_DOCUMENTS ? `; one run holds ${number(MAX_RUN_DOCUMENTS)}` : ''}.</small>
        </div>
        {problems.length > 0 && <p className="rounded-md border bg-muted p-3 text-xs">Fix {problems[0][0]}: {problems[0][1]}</p>}
        {!checked.length && <p className="rounded-md border bg-muted p-3 text-xs">Check at least one question.</p>}
        <Button className="w-full h-11" disabled={blocked} onClick={() => run.mutate()}>{run.isPending ? <Spinner /> : <Play />}{label}</Button>
        {busy && <p className="text-xs leading-relaxed text-muted-foreground">A run is going. It shows at the top of the page.</p>}
        {available.error && <ErrorBox error={available.error} />}{run.error && <ErrorBox error={run.error} />}
        <p className="text-xs leading-relaxed text-muted-foreground">{recording ? 'A saved run does not change what search returns until you commit it.' : 'A preview is not saved and cannot be committed.'}</p>
      </aside>
    </div>
  </>
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

function QuestionRow({ question, open, dirty, onToggle, edit, remove }: { question: LocalQuestion; open: boolean; dirty: boolean; onToggle: () => void; edit: (patch: Partial<LocalQuestion>) => void; remove: () => void }) {
  const problem = questionProblem(question)
  return <article className={`border-b last:border-b-0 ${open ? 'bg-muted' : ''} ${question.skip ? 'opacity-50' : ''}`}>
    <div className="flex items-center gap-3 px-4">
      <input type="checkbox" checked={!question.skip} onChange={event => edit({ skip: !event.target.checked })} aria-label={`Run ${question.id}`} />
      <button type="button" className="grid min-w-0 flex-1 grid-cols-[minmax(0,1fr)_auto_16px] items-center gap-3 py-3 text-left lg:grid-cols-[minmax(110px,190px)_auto_minmax(0,1fr)_auto_16px]" onClick={onToggle} aria-expanded={open}>
        <span className="truncate font-mono text-sm font-semibold">{question.id}</span>
        <span className="whitespace-nowrap rounded-full bg-secondary px-2 py-1 text-xs">{isChoice(question) ? `Choice · ${(question.options || []).length}` : 'Yes / no'}</span>
        <span className="hidden truncate text-xs text-muted-foreground lg:block">{policyShort(question)}</span>
        {problem ? <span className="hidden rounded bg-secondary px-2 py-1 text-xs lg:block text-destructive">fix</span> : dirty ? <span className="hidden rounded bg-secondary px-2 py-1 text-xs lg:block">unsaved</span> : <span className="hidden rounded bg-secondary px-2 py-1 text-xs lg:block text-muted-foreground">v{question.version}</span>}
        <ChevronDown className="size-4 text-muted-foreground" />
      </button>
    </div>
    {open && <QuestionEditor question={question} edit={edit} remove={remove} problem={problem} />}
  </article>
}

function QuestionEditor({ question, edit, remove, problem }: { question: LocalQuestion; edit: (patch: Partial<LocalQuestion>) => void; remove: () => void; problem: string }) {
  const choice = isChoice(question)
  const options = question.options || []
  const setOption = (index: number, patch: Partial<ChoiceOption>) => edit({ options: options.map((option, at) => at === index ? { ...option, ...patch } : option) })
  const setKind = (kind: 'noul' | 'choice') => edit(kind === 'choice'
    ? { kind, options: options.length ? options : blankChoice(0).options }
    : { kind, options: [] })
  return <div className="grid gap-4 px-4 pb-4 lg:pl-10">
    <div className="grid items-end gap-4 lg:grid-cols-[minmax(140px,220px)_auto_auto]">
      <label className="grid min-w-0 gap-2 text-xs font-medium [&_[data-slot=select-trigger]]:w-full"><span>Id</span><Input value={question.id} onChange={event => edit({ id: event.target.value })} /></label>
      <div className="grid min-w-0 gap-2 text-xs font-medium [&_[data-slot=select-trigger]]:w-full"><span>Kind</span><Segmented label="Question kind" value={question.kind || 'noul'} onChange={setKind} options={[['noul', 'Yes / no'], ['choice', 'Choice']]} /></div>
      {!choice && <div className="grid min-w-0 gap-2 text-xs font-medium [&_[data-slot=select-trigger]]:w-full"><span>When yes</span><Segmented label="Action" value={question.action} onChange={action => edit({ action })} options={actionOptions} /></div>}
    </div>
    <label className="grid min-w-0 gap-2 text-xs font-medium [&_[data-slot=select-trigger]]:w-full"><span>{choice ? 'Question' : 'Question (answered yes or no)'}</span><Textarea rows={2} value={question.instructions} placeholder={choice ? 'What is this text mainly?' : 'Does `text` …?'} onChange={event => edit({ instructions: event.target.value })} /></label>
    {choice && <div className="grid min-w-0 gap-2 text-xs font-medium [&_[data-slot=select-trigger]]:w-full"><span>Options</span>
      <div className="grid gap-3">
        {options.map((option, index) => <div className="grid grid-cols-[minmax(0,1fr)_auto] items-start gap-2 [&_textarea]:col-span-full lg:grid-cols-[minmax(0,1fr)_auto_auto]" key={index}>
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
  return <div className="grid grid-cols-1 gap-3 rounded-md border bg-muted p-3 sm:grid-cols-2 [&>p]:col-span-full [&_label]:grid [&_label]:gap-1 [&_label]:text-xs [&_input]:w-full">
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
