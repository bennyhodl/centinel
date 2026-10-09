import React, { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { createFileRoute, useNavigate, useSearch } from '@tanstack/react-router'
import { ChevronLeft, ChevronRight, Search as SearchIcon, X } from 'lucide-react'
import { api, corpusParams, type CorpusFacets, type CorpusFilters, type Document, type Question } from '../api'
import { classifierOptions } from '../classify'
import { characters, compact, number, tail } from '../format'
import { classificationBadges, hasScores } from '../policy'
import { DocumentLink, Empty, ErrorBox, PageHeader, shortSha } from '../ui'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'

export const Route = createFileRoute('/')({
  validateSearch: (search: Record<string, unknown>) => ({
    text: String(search.text || ''), address: String(search.address || ''),
    page: Math.max(1, Number(search.page || 1)), source: String(search.source || ''),
    usage: String(search.usage || 'all'), classifier: String(search.classifier || ''),
    minScore: String(search.minScore || '0.5'), maxScore: String(search.maxScore || ''),
  }),
  component: SearchPage,
})

type Search = ReturnType<typeof Route.useSearch>

/** One colour per kind of filter, so a chip says what it narrows by. */
const tone = {
  classifier: { chip: 'bg-flame-soft text-flame-ink shadow-[inset_0_0_0_1px_#F0D6B0]', dot: 'bg-flame', bar: 'bg-flame', faint: 'bg-[#EBD3B4]', label: 'text-flame-ink' },
  source: { chip: 'bg-slate-soft text-slate shadow-[inset_0_0_0_1px_#C9D6E2]', dot: 'bg-slate', bar: 'bg-slate', faint: 'bg-[#C9D6E2]', label: 'text-slate' },
  usage: { chip: 'bg-moss-soft text-moss shadow-[inset_0_0_0_1px_#C8DBC3]', dot: 'bg-moss', bar: 'bg-moss', faint: 'bg-[#C8DBC3]', label: 'text-moss' },
  words: { chip: 'bg-parchment text-foreground shadow-[inset_0_0_0_1px_var(--rule)]', dot: 'bg-foreground', bar: 'bg-foreground', faint: 'bg-rule', label: 'text-foreground' },
} as const
type Tone = keyof typeof tone

const sourcesIn = (value: string) => value.split(',').map(source => source.trim()).filter(Boolean)

function SearchPage() {
  const search = useSearch({ from: '/' })
  const navigate = useNavigate({ from: '/' })
  const [draft, setDraft] = useState(search.text)
  const filters: CorpusFilters = { search: search.text, address: search.address, source: search.source, usage: search.usage, classifier: search.classifier, min_score: search.minScore, max_score: search.maxScore }
  const params = corpusParams(filters, search.page, 25)
  const query = useQuery({ queryKey: ['corpus', params.toString()], queryFn: () => api.corpus(params) })
  const questions = useQuery({ queryKey: ['questions'], queryFn: api.questions })
  const set = (next: Partial<Search>) => navigate({ search: previous => ({ ...previous, ...next }) })
  const submit = (event: React.FormEvent) => { event.preventDefault(); set({ text: draft.trim(), page: 1 }) }
  const pages = Math.max(1, Math.ceil((query.data?.total || 0) / (query.data?.page_size || 25)))
  const saved = questions.data?.questions || []
  const classifiers = classifierOptions(saved)
  const classifierLabel = classifiers.find(([key]) => key === search.classifier)?.[1] || search.classifier
  const chosen = sourcesIn(search.source)
  const data = query.data

  return <>
    <PageHeader title="Search" detail={data ? `${number(data.total)} documents · ${characters(data.total_chars)} of text. ${number(data.pending)} waiting on a classifier.` : 'Reading the index…'} />
    <form className="flex gap-2" onSubmit={submit}>
      <label className="flex h-12 flex-1 items-center gap-3 rounded-xl border border-input bg-background px-4 shadow-[0_1px_2px_rgba(26,23,18,0.04)] focus-within:border-flame focus-within:ring-[3px] focus-within:ring-flame/20">
        <SearchIcon className="size-[18px] shrink-0 text-muted-foreground" />
        <input className="h-full flex-1 bg-transparent text-[15px] outline-none placeholder:text-muted-foreground" value={draft} onChange={event => setDraft(event.target.value)} placeholder="Words inside the documents" aria-label="Full-text search" />
      </label>
      <Button type="submit" className="h-12 rounded-xl px-6">Search</Button>
    </form>
    <div className="mt-3 flex min-h-8 flex-wrap items-center gap-2">
      {search.text && <Chip tone="words" label="Words" value={search.text} onClear={() => { setDraft(''); set({ text: '', page: 1 }) }} />}
      {search.address && <Chip tone="words" label="Address" value={search.address} onClear={() => set({ address: '', page: 1 })} />}
      {chosen.map(source => <Chip key={source} tone="source" label="Source" value={source} onClear={() => set({ source: chosen.filter(s => s !== source).join(','), page: 1 })} />)}
      {search.usage !== 'all' && <Chip tone="usage" label="Usage" value={search.usage} onClear={() => set({ usage: 'all', page: 1 })} />}
      {search.classifier && <Chip tone="classifier" label={classifierLabel} value={`${search.minScore || '0'} – ${search.maxScore || '1'}`} onClear={() => set({ classifier: '', page: 1 })} />}
      <span className="ml-auto text-[13px] text-muted-foreground">{data ? `${number(data.total)} documents` : ''}</span>
    </div>

    <div className="mt-5 flex flex-col gap-10 lg:flex-row lg:items-start">
      <section className="min-w-0 flex-1">
        <div className="flex h-8 items-center gap-6 border-b border-foreground text-[11px] font-semibold uppercase tracking-[0.08em] text-muted-foreground">
          <span className="flex-1">Document</span><span className="hidden w-56 md:block">Scores</span><span className="w-16 text-right">Blob</span>
        </div>
        {query.error ? <ErrorBox error={query.error} /> : data?.documents.map(doc => <ResultRow key={`${doc.source}:${doc.resource}:${doc.derived_sha}`} doc={doc} questions={saved} />)}
        {data && !data.documents.length && <Empty>No documents match. Clear a filter and try again.</Empty>}
        {data && data.total > 0 && <div className="flex items-center justify-between py-4 text-[13px] text-muted-foreground">
          <span>Page {search.page} of {number(pages)}</span>
          <span className="flex gap-2"><Button variant="outline" size="sm" disabled={search.page === 1} onClick={() => set({ page: search.page - 1 })}><ChevronLeft />Previous</Button><Button variant="outline" size="sm" disabled={search.page >= pages} onClick={() => set({ page: search.page + 1 })}>Next<ChevronRight /></Button></span>
        </div>}
      </section>
      <Refine search={search} set={set} classifiers={classifiers} sources={data?.sources || []} facets={data?.facets} />
    </div>
  </>
}

function Chip({ tone: kind, label, value, onClear }: { tone: Tone; label: string; value: string; onClear: () => void }) {
  return <span className={`inline-flex h-[30px] items-center gap-1.5 rounded-full px-2.5 text-[13px] ${tone[kind].chip}`}>
    <span className={`size-1.5 rounded-full ${tone[kind].dot}`} />
    <span className="opacity-75">{label}</span><b className="max-w-64 truncate font-semibold">{value}</b>
    <button type="button" aria-label={`Clear ${label} ${value}`} className="opacity-60 hover:opacity-100" onClick={onClear}><X className="size-3.5" /></button>
  </span>
}

function ResultRow({ doc, questions }: { doc: Document; questions: Question[] }) {
  const badges = classificationBadges(doc.classifications, questions).slice(0, 3)
  return <div className={`group flex items-start gap-6 border-b py-4 ${doc.excluded ? 'opacity-55' : ''}`}>
    <DocumentLink doc={doc} className="grid min-w-0 flex-1 gap-1.5">
      <b className="text-[15px] leading-5 font-semibold group-hover:underline">{doc.title || tail(doc.resource)}</b>
      <span className="flex min-w-0 items-center gap-2 text-xs text-muted-foreground">
        <span className={`shrink-0 rounded px-1.5 py-px font-medium ${tone.source.chip}`}>{doc.source}</span>
        <span className="truncate">{doc.resource}</span>
        <span className="shrink-0">· {characters(doc.chars)}</span>
        {doc.excluded && <span className="shrink-0 rounded px-1.5 py-px font-medium text-destructive shadow-[inset_0_0_0_1px_#E2B1A8]">excluded</span>}
      </span>
    </DocumentLink>
    <span className="hidden w-56 flex-wrap gap-1.5 pt-0.5 md:flex">
      {badges.length ? badges.map(badge => <span key={badge.key} className={`rounded px-2 py-0.5 text-xs font-semibold ${badge.tone === 'muted' ? 'text-muted-foreground shadow-[inset_0_0_0_1px_var(--rule)]' : tone.classifier.chip}`}>{badge.text}</span>) : <span className="text-xs text-muted-foreground">{hasScores(doc.classifications) ? 'No tags' : 'Not classified'}</span>}
    </span>
    <span className="w-16 pt-0.5 text-right font-mono text-xs text-muted-foreground">{shortSha(doc.blob_sha)}</span>
  </div>
}

const usageOptions: Array<[string, string]> = [['all', 'All'], ['included', 'Included'], ['excluded', 'Excluded'], ['pending', 'Pending']]

/** Narrow a search by what the classifiers said, where it was collected, and its usage. */
function Refine({ search, set, classifiers, sources, facets }: { search: Search; set: (next: Partial<Search>) => void; classifiers: Array<[string, string]>; sources: string[]; facets?: CorpusFacets }) {
  const [address, setAddress] = useState(search.address)
  const chosen = sourcesIn(search.source)
  const sourceCounts = facets?.sources || {}
  const sourceMax = Math.max(1, ...Object.values(sourceCounts))
  const usageCounts = facets?.usage
  const usageAll = usageCounts ? (usageCounts.included || 0) + (usageCounts.excluded || 0) : 0
  const toggleSource = (source: string) => set({ source: (chosen.includes(source) ? chosen.filter(s => s !== source) : [...chosen, source]).join(','), page: 1 })

  return <aside className="grid w-full shrink-0 gap-8 lg:w-[288px]">
    <section className="grid gap-1">
      <Rule kind="classifier" aside={search.classifier ? <button type="button" onClick={() => set({ classifier: '', page: 1 })}>Reset</button> : undefined}>Classifiers</Rule>
      {classifiers.map(([key, label]) => {
        const bins = facets?.scores[key]
        const active = key === search.classifier
        const lo = Number(search.minScore) || 0, hi = search.maxScore === '' ? 1 : Number(search.maxScore)
        return <div key={key} className={`grid gap-2 rounded-lg px-2 py-2 ${active ? 'bg-[#FFFBF4] shadow-[inset_0_0_0_1px_#F0D6B0]' : 'hover:bg-[#FBF9F4]'}`}>
          <button type="button" className="flex items-baseline justify-between gap-2 text-left" onClick={() => set(active ? { classifier: '', page: 1 } : { classifier: key, page: 1 })}>
            <span className={`truncate text-sm ${active ? 'font-semibold' : ''}`}>{label}</span>
            <span className="shrink-0 font-mono text-xs text-muted-foreground">{active ? `${search.minScore || '0'} – ${search.maxScore || '1'}` : bins ? compact(bins.reduce((a, b) => a + b, 0)) : ''}</span>
          </button>
          {bins && <Histogram bins={bins} from={active ? lo : 0} to={active ? hi : 1} active={active} />}
          {active && <div className="flex gap-2">
            <Input aria-label="Lowest score" type="number" min="0" max="1" step="0.05" className="h-8 bg-background font-mono text-xs" value={search.minScore} onChange={event => set({ minScore: event.target.value, page: 1 })} />
            <Input aria-label="Highest score" type="number" min="0" max="1" step="0.05" className="h-8 bg-background font-mono text-xs" placeholder="1" value={search.maxScore} onChange={event => set({ maxScore: event.target.value, page: 1 })} />
          </div>}
        </div>
      })}
      {!classifiers.length && <p className="py-2 text-sm text-muted-foreground">No classifiers yet. Write one on Classify.</p>}
    </section>

    <section className="grid gap-1">
      <Rule kind="usage">Usage</Rule>
      {usageOptions.map(([value, label]) => {
        const count = !usageCounts ? undefined : value === 'all' ? usageAll : usageCounts[value]
        const active = search.usage === value
        return <button type="button" key={value} onClick={() => set({ usage: value, page: 1 })} className={`grid gap-1 rounded-lg px-2 py-1.5 text-left ${active ? 'bg-moss-soft' : 'hover:bg-[#FBF9F4]'}`}>
          <span className="flex justify-between text-sm"><span className={active ? `font-semibold ${tone.usage.label}` : ''}>{label}</span>{count != null && <span className="font-mono text-xs text-muted-foreground">{number(count)}</span>}</span>
          {count != null && <Meter value={count} max={Math.max(1, usageAll)} kind="usage" strong={active} />}
        </button>
      })}
    </section>

    <section className="grid gap-1">
      <Rule kind="source" aside={chosen.length ? <button type="button" onClick={() => set({ source: '', page: 1 })}>All</button> : `${sources.length}`}>Sources</Rule>
      <div className="grid max-h-96 overflow-y-auto">
        {sources.map(source => {
          const active = chosen.includes(source)
          const count = sourceCounts[source]
          return <button type="button" key={source} aria-pressed={active} onClick={() => toggleSource(source)} className={`grid gap-1 rounded-lg px-2 py-1.5 text-left ${active ? 'bg-slate-soft' : 'hover:bg-[#FBF9F4]'}`}>
            <span className="flex items-center justify-between gap-2 text-sm">
              <span className="flex min-w-0 items-center gap-2"><span className={`grid size-3.5 shrink-0 place-items-center rounded-[4px] ${active ? 'bg-slate text-white' : 'shadow-[inset_0_0_0_1.5px_#B9C6D3]'}`}>{active && <svg viewBox="0 0 24 24" className="size-2.5" fill="none" stroke="currentColor" strokeWidth="4"><path d="M5 12l5 5L20 7" /></svg>}</span><span className={`truncate ${active ? `font-semibold ${tone.source.label}` : ''}`}>{source}</span></span>
              {count != null && <span className="font-mono text-xs text-muted-foreground">{number(count)}</span>}
            </span>
            {count != null && <Meter value={count} max={sourceMax} kind="source" strong={active} />}
          </button>
        })}
      </div>
    </section>

    <form className="grid gap-2" onSubmit={event => { event.preventDefault(); set({ address: address.trim(), page: 1 }) }}>
      <Rule kind="words">Address or title</Rule>
      <Input className="mt-1 h-8 text-[13px]" value={address} onChange={event => setAddress(event.target.value)} placeholder="tampa.gov/agenda" />
    </form>
  </aside>
}

function Rule({ kind, children, aside }: { kind: Tone; children: React.ReactNode; aside?: React.ReactNode }) {
  return <div className="mb-1 flex h-8 items-center justify-between border-b border-foreground">
    <span className="inline-flex items-center gap-2 text-[11px] font-semibold uppercase tracking-[0.08em] text-muted-foreground"><span className={`size-1.5 rounded-full ${tone[kind].dot}`} />{children}</span>
    {aside && <span className="text-xs text-muted-foreground">{aside}</span>}
  </div>
}

function Meter({ value, max, kind, strong }: { value: number; max: number; kind: Tone; strong: boolean }) {
  return <span className="block h-1 rounded-full bg-[#EFE9DC]"><i className={`block h-full rounded-full ${strong ? tone[kind].bar : tone[kind].faint}`} style={{ width: `${Math.max(value ? 2 : 0, value / max * 100)}%` }} /></span>
}

/** How a classifier's scores spread over the matching documents, a bar per tenth, with the chosen band lit. */
function Histogram({ bins, from, to, active }: { bins: number[]; from: number; to: number; active: boolean }) {
  const top = Math.max(1, ...bins)
  return <span className="flex h-7 items-end gap-[2px]" aria-hidden>
    {bins.map((count, tenth) => {
      const inside = tenth / 10 + 0.05 >= from && tenth / 10 + 0.05 <= to
      return <i key={tenth} title={`${(tenth / 10).toFixed(1)}–${((tenth + 1) / 10).toFixed(1)}: ${count}`} className={`flex-1 rounded-t-[2px] ${active && inside ? tone.classifier.bar : active ? 'bg-rule' : tone.classifier.faint}`} style={{ height: `${Math.max(count ? 8 : 3, count / top * 100)}%` }} />
    })}
  </span>
}
