import { useEffect, useRef, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Check, ListPlus, Play, Plus, Trash2, X } from 'lucide-react'
import { api, corpusParams, type ChoiceOption, type CorpusFilters, type Document, type Preset, type Question, type QuestionAction, type RunDetailQuery } from './api'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { compact, money, number, plural, seconds, tail } from './format'
import { LiveRun, useRunDetail } from './live'
import { decisionLabels, decisionOf, estimateRun, isChoice, missingOther, policyShort, questionProblem, questionSnapshot, tagsOf } from './policy'
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
  const [panel, setPanel] = useState<'edit' | 'test' | 'run'>('edit')
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

  const select = (localKey: string) => { setOpenKey(localKey); setPanel('edit') }
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
  const remove = (localKey: string) => setQuestions(current => {
    const gone = current.find(question => question.localKey === localKey)
    const tags = new Set(gone ? outcomesOf(gone).flatMap(outcome => outcome.tag ? [outcome.tag] : []) : [])
    return current.filter(question => question.localKey !== localKey).map(question => question.when && tags.has(question.when) ? { ...question, when: undefined } : question)
  })
  const follow = (tag: string) => {
    const fresh = withKeys([{ ...blankNoul(questions.length), id: uniqueId(questions, `${tag.replace(':', '_')}_follow_up`), when: tag }])
    setQuestions(current => [...current, ...fresh])
    select(fresh[0].localKey)
  }
  const selected = questions.find(question => question.localKey === openKey)
  const test = useTest(checked.map(stripKey), model, date)

  return <>
    <div ref={top} />
    <PageHeader title="Classify" detail="Jev answers the questions. The tree decides what happens. Test a document and watch the path it takes.">
      <div className="flex shrink-0 gap-2">
        <AddMenu onAdd={add} count={questions.length} existing={questions.map(question => question.id)} presets={shipped.data?.presets || []} />
        <Button variant="outline" size="sm" disabled={save.isPending || !dirty || problems.length > 0} onClick={() => save.mutate()}>{save.isPending ? <Spinner /> : <Check />}{dirty ? 'Save tree' : 'Saved'}</Button>
      </div>
    </PageHeader>
    {active && <LiveRun key={active.id} id={active.id} preview={active.preview} onDismiss={() => setActive(null)} />}
    {active?.preview && <PreviewResults id={active.id} />}
    {shipped.error && <ErrorBox error={shipped.error} />}{saved.error && <ErrorBox error={saved.error} />}{save.error && <ErrorBox error={save.error} />}

    <div className="flex flex-col gap-5 xl:flex-row xl:items-start">
      <section className="min-h-[560px] min-w-0 flex-1 overflow-auto rounded-xl border bg-[#FBF8F1] bg-[radial-gradient(#E2DACB_1px,transparent_1px)] [background-size:18px_18px] p-5">
        <TestBar test={test} onPick={() => setPanel('test')} />
        {questions.length ? <Tree questions={questions} selected={openKey} answers={test.answers} isDirty={isDirty} onSelect={select} onFollow={follow} onToggle={(localKey, run) => edit(localKey, { skip: !run })} /> : <Empty>Add the junk gate or another preset to start the tree.</Empty>}
        <div className="mt-8 flex flex-wrap items-center gap-4 text-xs text-muted-foreground">
          <span className="inline-flex items-center gap-1.5"><i className="h-[3px] w-4 rounded bg-flame" />path Jev took</span>
          <span className="inline-flex items-center gap-1.5"><i className="w-4 border-t-[1.5px] border-dashed border-[#CFC6B5]" />off the path</span>
          <span>Hover an answer and press + to ask a follow-up of the documents that get it.</span>
        </div>
      </section>

      <aside className="grid w-full shrink-0 gap-4 rounded-xl border bg-card p-4 xl:sticky xl:top-5 xl:w-[380px]">
        <div className="inline-flex w-fit gap-0.5 rounded-lg bg-parchment p-1" role="tablist">
          {(['edit', 'test', 'run'] as const).map(name => <button type="button" role="tab" aria-selected={panel === name} key={name} onClick={() => setPanel(name)} className={`h-[30px] rounded-md px-3 text-[13px] ${panel === name ? 'bg-background font-semibold shadow-xs' : 'text-muted-foreground hover:text-foreground'}`}>{{ edit: 'Question', test: 'Test', run: 'Run' }[name]}</button>)}
        </div>
        {panel === 'edit' && (selected
          ? <QuestionEditor question={selected} questions={questions} edit={patch => editQuestion(selected.localKey, patch)} remove={() => { remove(selected.localKey); setOpenKey('') }} problem={questionProblem(selected)} />
          : <p className="text-sm text-muted-foreground">Click a question in the tree to edit it.</p>)}
        {panel === 'test' && <TestPanel test={test} />}
        {panel === 'run' && <>
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
        </>}
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

/** One answer a question can give, and the tag a follow-up hangs off. A noul's "no" tags nothing. */
type Branch = { label: string; tag?: string; action: QuestionAction }

export function outcomesOf(question: Question): Branch[] {
  return isChoice(question)
    ? (question.options || []).map(option => ({ label: option.id, tag: `${question.id}:${option.id}`, action: option.action }))
    : [{ label: 'yes', tag: question.id, action: question.action }, { label: 'no', action: 'keep' }]
}

/** Jev's probability for one answer, from a document's stored answers. */
function probabilityOf(question: Question, branch: Branch, answers: Record<string, number>) {
  if (isChoice(question)) return answers[`${question.id}:${branch.label}`]
  const yes = answers[question.id]
  return yes == null ? undefined : branch.label === 'yes' ? yes : 1 - yes
}

/** The answer Jev gave: a choice's likeliest option, or a noul's side of one half. */
function answered(question: Question, answers?: Record<string, number>) {
  if (!answers) return undefined
  const scored = outcomesOf(question).map(branch => [branch.label, probabilityOf(question, branch, answers)] as const).filter(([, p]) => p != null)
  return scored.sort((a, b) => (b[1] ?? 0) - (a[1] ?? 0))[0]?.[0]
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

type TreeProps = {
  questions: LocalQuestion[]
  selected: string
  answers?: Record<string, number>
  isDirty: (question: LocalQuestion) => boolean
  onSelect: (localKey: string) => void
  onFollow: (tag: string) => void
  onToggle: (localKey: string, run: boolean) => void
}

/** Questions on the left, their answers to the right, follow-ups hanging off an answer. */
function Tree(props: TreeProps) {
  const tags = new Set(props.questions.flatMap(question => outcomesOf(question).flatMap(branch => branch.tag ? [branch.tag] : [])))
  const roots = props.questions.filter(question => !question.when || !tags.has(question.when))
  return <div className="flex flex-col gap-10">{roots.map(question => <Node key={question.localKey} question={question} reached {...props} />)}</div>
}

function Node({ question, reached, ...props }: TreeProps & { question: LocalQuestion; reached: boolean }) {
  const lit = reached ? answered(question, props.answers) : undefined
  const tested = Boolean(props.answers)
  const dim = tested && !reached
  return <div className="flex items-start">
    <QuestionCard question={question} selected={props.selected === question.localKey} dirty={props.isDirty(question)} dim={dim} live={tested && reached} onSelect={() => props.onSelect(question.localKey)} onToggle={run => props.onToggle(question.localKey, run)} />
    <span className={`mt-[25px] w-4 shrink-0 border-t-[1.5px] ${lit ? 'border-flame' : 'border-[#D8CFBD]'}`} />
    <div className="flex flex-col gap-1.5 pt-2">
      {outcomesOf(question).map((branch, index, all) => {
        const on = lit === branch.label
        const children = branch.tag ? props.questions.filter(child => child.when === branch.tag) : []
        const p = props.answers ? probabilityOf(question, branch, props.answers) : undefined
        const first = index === 0, last = index === all.length - 1
        return <div key={branch.label} className="group/branch relative flex items-start">
          {all.length > 1 && <span className={`absolute left-0 border-l-[1.5px] border-[#D8CFBD] ${first ? 'top-[17px]' : '-top-1.5'} ${last ? (first ? 'h-0' : 'h-[23px]') : 'bottom-0'}`} />}
          <span className={`mt-[17px] h-0 w-5 shrink-0 border-t-[1.5px] ${on ? 'border-flame' : dim ? 'border-dashed border-[#CFC6B5]' : 'border-[#D8CFBD]'}`} />
          <span className={`flex h-[34px] w-44 shrink-0 items-center gap-2 rounded-lg px-2.5 text-[13px] ${on ? 'bg-flame-soft font-bold shadow-[0_0_0_1.5px_var(--flame)]' : 'bg-background shadow-[0_0_0_1px_var(--rule)]'} ${dim ? 'opacity-55' : ''}`}>
            <span className="min-w-0 flex-1 truncate">{branch.label}</span>
            <span className={`text-[10px] ${branch.action === 'exclude' ? 'text-destructive' : on ? 'text-flame-ink' : 'text-muted-foreground'}`}>{children.length ? '→ next' : actionWord[branch.action]}</span>
            {p != null && <span className={`w-9 text-right font-mono ${on ? 'text-flame-ink' : 'text-muted-foreground'}`}>{Math.round(p * 100)}%</span>}
          </span>
          {branch.tag && !children.length && <button type="button" aria-label={`Ask a follow-up on ${branch.label}`} onClick={() => props.onFollow(branch.tag!)} className="ml-2 mt-[5px] grid size-6 place-items-center rounded-md border border-dashed border-[#CFC6B5] bg-background text-muted-foreground opacity-0 group-hover/branch:opacity-100 focus:opacity-100 hover:text-foreground"><Plus className="size-3.5" /></button>}
          {children.length > 0 && <div className="flex flex-col gap-6">
            {children.map(child => <div key={child.localKey} className="flex items-start"><span className={`mt-[17px] h-0 w-8 shrink-0 border-t-[1.5px] ${on ? 'border-flame' : 'border-dashed border-[#CFC6B5]'}`} /><Node question={child} reached={reached && on} {...props} /></div>)}
            <button type="button" onClick={() => props.onFollow(branch.tag!)} className="ml-8 hidden h-8 w-56 items-center justify-center rounded-lg border border-dashed border-[#CFC6B5] text-xs text-muted-foreground group-hover/branch:flex hover:text-foreground">+ Another follow-up on “{branch.label}”</button>
          </div>}
        </div>
      })}
    </div>
  </div>
}

const actionWord: Record<QuestionAction, string> = { exclude: 'exclude', tag: 'tag', keep: 'keep' }

function QuestionCard({ question, selected, dirty, dim, live, onSelect, onToggle }: { question: LocalQuestion; selected: boolean; dirty: boolean; dim: boolean; live: boolean; onSelect: () => void; onToggle: (run: boolean) => void }) {
  const problem = questionProblem(question)
  return <div role="button" tabIndex={0} onClick={onSelect} onKeyDown={event => event.key === 'Enter' && onSelect()} className={`grid w-56 shrink-0 cursor-pointer gap-1.5 rounded-[10px] bg-background px-3.5 py-3 text-left ${selected ? 'shadow-[0_0_0_2px_var(--flame),0_4px_14px_rgba(26,23,18,0.08)]' : live ? 'shadow-[0_0_0_1.5px_var(--flame),0_4px_14px_rgba(26,23,18,0.08)]' : 'shadow-[0_0_0_1.5px_var(--foreground),0_4px_14px_rgba(26,23,18,0.08)]'} ${dim || question.skip ? 'opacity-55' : ''}`}>
    <div className="flex items-center gap-2">
      <input type="checkbox" checked={!question.skip} onClick={event => event.stopPropagation()} onChange={event => onToggle(event.target.checked)} aria-label={`Run ${question.id}`} className="accent-foreground" />
      <span className={`shrink-0 text-[10px] font-bold tracking-[0.1em] ${problem ? 'text-destructive' : dirty ? 'text-flame-ink' : 'text-muted-foreground'}`}>{isChoice(question) ? 'CHOICE' : 'NOUL'} · {problem ? 'fix' : dirty ? 'unsaved' : `v${question.version}`}</span>
      <span className="ml-auto min-w-0 truncate font-mono text-[11px] text-muted-foreground">{question.id}</span>
    </div>
    <span className="line-clamp-2 text-[15px] leading-[19px] font-semibold">{question.instructions || 'Write the question'}</span>
    <span className="text-xs text-muted-foreground">{policyShort(question)}</span>
  </div>
}

type Test = ReturnType<typeof useTest>

/**
 * One document through the tree as a preview run: nothing is saved, and draft questions
 * are allowed. Jev answers every checked question today; the tree shows which ones the
 * document would have reached.
 */
function useTest(questions: Question[], model: string, date: string) {
  const [doc, setDoc] = useState<Document | null>(null)
  const start = useMutation({
    mutationFn: (picked: Document) => api.run({ documents: [{ source: picked.source, resource: picked.resource, derived_sha: picked.derived_sha }], questions, model, evaluation_date: date, record: false, settings: { concurrency: 1 } }),
  })
  const detail = useRunDetail(start.data?.id || '', { page: 1, page_size: 1 }, 700)
  const result = detail.data?.status !== 'running' ? detail.data?.results?.[0] : undefined
  const pick = (picked: Document) => { setDoc(picked); start.mutate(picked) }
  return { doc, pick, result, answers: result && !result.error ? result.answers : undefined, running: start.isPending || detail.data?.status === 'running', error: start.error || detail.error, again: () => doc && start.mutate(doc) }
}

function TestBar({ test, onPick }: { test: Test; onPick: () => void }) {
  return <div className="mb-6 inline-flex h-9 items-center gap-2.5 rounded-full bg-foreground py-0 pr-1.5 pl-3 shadow-[0_4px_14px_rgba(26,23,18,0.18)]">
    <span className={`size-2 rounded-full ${test.doc ? 'bg-flame shadow-[0_0_0_3px_rgba(200,118,30,0.35)]' : 'bg-[#6B6458]'}`} />
    <span className="text-[13px] text-[#B9AE98]">{test.running ? 'Asking Jev…' : test.doc ? 'Testing' : 'No document under test'}</span>
    {test.doc && <span className="max-w-80 truncate text-[13px] font-semibold text-parchment">{test.doc.title || tail(test.doc.resource)}</span>}
    <button type="button" onClick={onPick} className="h-[26px] rounded-full bg-[#3A352D] px-2.5 text-xs text-parchment">{test.doc ? 'Pick another' : 'Pick a document'} ▾</button>
  </div>
}

function TestPanel({ test }: { test: Test }) {
  const [text, setText] = useState('')
  const params = corpusParams({ search: '', address: text.trim(), source: '', usage: 'all', classifier: '', min_score: '', max_score: '' }, 1, 8)
  const found = useQuery({ queryKey: ['corpus', 'test-pick', params.toString()], queryFn: () => api.corpus(params) })
  return <div className="grid gap-3">
    <Input value={text} onChange={event => setText(event.target.value)} placeholder="Find a document by address or title" />
    <div className="grid">
      {(found.data?.documents || []).map(doc => <button type="button" key={`${doc.source}:${doc.resource}`} onClick={() => test.pick(doc)} className={`grid gap-0.5 border-b py-2 text-left hover:bg-[#FBF9F4] ${test.doc?.resource === doc.resource ? 'font-semibold' : ''}`}>
        <span className="truncate text-sm">{doc.title || tail(doc.resource)}</span><span className="truncate text-xs text-muted-foreground">{doc.source} · {doc.resource}</span>
      </button>)}
    </div>
    {test.error && <ErrorBox error={test.error} />}
    {test.result && <div className="grid gap-2 rounded-[10px] border p-3.5">
      <span className="text-[10px] font-bold tracking-[0.1em] text-muted-foreground">OUTCOME FOR THIS DOCUMENT</span>
      <b className="text-[15px]">{test.result.error ? 'Jev could not answer' : `${decisionLabels[decisionOf(test.result)]}${tagsOf(test.result).length ? ` · ${tagsOf(test.result).join(', ')}` : ''}`}</b>
      <span className="text-xs text-muted-foreground">{test.result.error || `${seconds(test.result.duration_ms)} · a preview, nothing saved.`}</span>
      <Button size="sm" variant="outline" disabled={test.running} onClick={test.again}>Ask again with these questions</Button>
    </div>}
    <p className="text-xs leading-relaxed text-muted-foreground">Jev answers every checked question today. Branches off the lit path show what it would have said; once runs follow the tree, they will not be asked.</p>
  </div>
}
