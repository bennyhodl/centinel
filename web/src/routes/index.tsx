import React, { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { createFileRoute, useNavigate, useSearch } from '@tanstack/react-router'
import { ChevronLeft, ChevronRight, Search as SearchIcon, X } from 'lucide-react'
import { api, corpusParams, type CorpusFilters, type Document, type Question } from '../api'
import { classifierOptions } from '../classify'
import { characters, number, tail } from '../format'
import { classificationBadges, hasScores } from '../policy'
import { DocumentLink, Empty, ErrorBox, PageHeader, SectionRule, shortSha } from '../ui'
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

  return <>
    <PageHeader title="Search" detail={query.data ? `${number(query.data.total)} documents · ${characters(query.data.total_chars)} of text. ${number(query.data.pending)} waiting on a classifier.` : 'Reading the index…'} />
    <form className="flex gap-2" onSubmit={submit}>
      <label className="flex h-11 flex-1 items-center gap-2.5 rounded-lg border border-input bg-background px-3.5 shadow-xs focus-within:border-ring focus-within:ring-[3px] focus-within:ring-ring/30">
        <SearchIcon className="size-[18px] shrink-0 text-muted-foreground" />
        <input className="h-full flex-1 bg-transparent text-[15px] outline-none placeholder:text-muted-foreground" value={draft} onChange={event => setDraft(event.target.value)} placeholder="Words inside the documents" aria-label="Full-text search" />
      </label>
      <Button type="submit" className="h-11 px-5">Search</Button>
    </form>
    <div className="mt-3 flex min-h-8 flex-wrap items-center gap-2">
      {search.text && <Chip label="Words" value={search.text} onClear={() => { setDraft(''); set({ text: '', page: 1 }) }} />}
      {search.address && <Chip label="Address" value={search.address} onClear={() => set({ address: '', page: 1 })} />}
      {search.source && <Chip label="Source" value={search.source} onClear={() => set({ source: '', page: 1 })} />}
      {search.usage !== 'all' && <Chip label="Usage" value={search.usage} onClear={() => set({ usage: 'all', page: 1 })} />}
      {search.classifier && <Chip label={classifierLabel} value={`${search.minScore || '0'} – ${search.maxScore || '1'}`} onClear={() => set({ classifier: '', page: 1 })} />}
      <span className="ml-auto text-[13px] text-muted-foreground">{query.data ? `${number(query.data.total)} documents` : ''}</span>
    </div>

    <div className="mt-5 flex flex-col gap-10 lg:flex-row lg:items-start">
      <section className="min-w-0 flex-1">
        <div className="flex h-8 items-center gap-6 border-b border-foreground text-[11px] font-semibold uppercase tracking-[0.08em] text-muted-foreground">
          <span className="flex-1">Document</span><span className="hidden w-56 md:block">Scores</span><span className="w-16 text-right">Blob</span>
        </div>
        {query.error ? <ErrorBox error={query.error} /> : query.data?.documents.map(doc => <ResultRow key={`${doc.source}:${doc.resource}:${doc.derived_sha}`} doc={doc} questions={saved} />)}
        {query.data && !query.data.documents.length && <Empty>No documents match. Clear a filter and try again.</Empty>}
        {query.data && query.data.total > 0 && <div className="flex items-center justify-between py-4 text-[13px] text-muted-foreground">
          <span>Page {search.page} of {number(pages)}</span>
          <span className="flex gap-2"><Button variant="outline" size="sm" disabled={search.page === 1} onClick={() => set({ page: search.page - 1 })}><ChevronLeft />Previous</Button><Button variant="outline" size="sm" disabled={search.page >= pages} onClick={() => set({ page: search.page + 1 })}>Next<ChevronRight /></Button></span>
        </div>}
      </section>
      <Refine search={search} set={set} classifiers={classifiers} sources={query.data?.sources || []} />
    </div>
  </>
}

function Chip({ label, value, onClear }: { label: string; value: string; onClear: () => void }) {
  return <span className="inline-flex h-[30px] items-center gap-1.5 rounded-full border px-2.5 text-[13px]">
    <span className="text-muted-foreground">{label}</span><b className="max-w-64 truncate font-semibold">{value}</b>
    <button type="button" aria-label={`Clear ${label}`} className="text-muted-foreground hover:text-foreground" onClick={onClear}><X className="size-3.5" /></button>
  </span>
}

function ResultRow({ doc, questions }: { doc: Document; questions: Question[] }) {
  const badges = classificationBadges(doc.classifications, questions).slice(0, 3)
  return <div className={`flex items-start gap-6 border-b py-4 ${doc.excluded ? 'opacity-55' : ''}`}>
    <DocumentLink doc={doc} className="group grid min-w-0 flex-1 gap-1">
      <b className="text-[15px] leading-5 font-semibold group-hover:underline">{doc.title || tail(doc.resource)}</b>
      <span className="truncate text-xs text-muted-foreground">{doc.source} · {doc.resource} · {characters(doc.chars)}{doc.excluded ? ' · excluded' : ''}</span>
    </DocumentLink>
    <span className="hidden w-56 flex-wrap gap-1.5 pt-0.5 md:flex">
      {badges.length ? badges.map(badge => <span key={badge.key} className={`rounded px-2 py-0.5 text-xs font-semibold ${badge.tone === 'muted' ? 'border text-muted-foreground' : 'bg-parchment text-foreground'}`}>{badge.text}</span>) : <span className="text-xs text-muted-foreground">{hasScores(doc.classifications) ? 'No tags' : 'Not classified'}</span>}
    </span>
    <span className="w-16 pt-0.5 text-right font-mono text-xs text-muted-foreground">{shortSha(doc.blob_sha)}</span>
  </div>
}

const usageOptions: Array<[string, string]> = [['all', 'All'], ['included', 'Included'], ['excluded', 'Excluded'], ['pending', 'Pending']]

/** Narrow a search by what the classifiers said, where it was collected, and its usage. */
function Refine({ search, set, classifiers, sources }: { search: Search; set: (next: Partial<Search>) => void; classifiers: Array<[string, string]>; sources: string[] }) {
  const [address, setAddress] = useState(search.address)
  return <aside className="grid w-full shrink-0 gap-7 lg:w-[272px]">
    <div className="grid gap-1">
      <SectionRule aside={search.classifier ? <button type="button" onClick={() => set({ classifier: '', page: 1 })}>Reset</button> : undefined}>Classifiers</SectionRule>
      {classifiers.map(([key, label]) => key === search.classifier
        ? <div key={key} className="grid gap-2 py-2">
          <div className="flex items-baseline justify-between"><b className="text-sm font-semibold">{label}</b><span className="font-mono text-xs">{search.minScore || '0'} – {search.maxScore || '1'}</span></div>
          <ScoreRange min={search.minScore} max={search.maxScore} />
          <div className="flex gap-2">
            <Input aria-label="Lowest score" type="number" min="0" max="1" step="0.05" className="h-8 font-mono text-xs" value={search.minScore} onChange={event => set({ minScore: event.target.value, page: 1 })} />
            <Input aria-label="Highest score" type="number" min="0" max="1" step="0.05" className="h-8 font-mono text-xs" placeholder="1" value={search.maxScore} onChange={event => set({ maxScore: event.target.value, page: 1 })} />
          </div>
        </div>
        : <button type="button" key={key} className="flex h-8 items-center justify-between text-left text-sm text-muted-foreground hover:text-foreground" onClick={() => set({ classifier: key, page: 1 })}><span className="truncate">{label}</span><span className="text-xs">+ filter</span></button>)}
      {!classifiers.length && <p className="py-2 text-sm text-muted-foreground">No classifiers yet. Write one on Classify.</p>}
    </div>
    <div className="grid gap-2">
      <SectionRule>Usage</SectionRule>
      <div className="flex flex-wrap gap-1 pt-1">{usageOptions.map(([value, label]) => <button type="button" key={value} onClick={() => set({ usage: value, page: 1 })} className={`h-7 rounded-md px-2.5 text-[13px] ${search.usage === value ? 'bg-foreground font-semibold text-background' : 'bg-parchment text-muted-foreground hover:text-foreground'}`}>{label}</button>)}</div>
    </div>
    <form className="grid gap-2" onSubmit={event => { event.preventDefault(); set({ address: address.trim(), page: 1 }) }}>
      <SectionRule>Address or title</SectionRule>
      <Input className="mt-1 h-8 text-[13px]" value={address} onChange={event => setAddress(event.target.value)} placeholder="tampa.gov/agenda" />
    </form>
    <div className="grid gap-1">
      <SectionRule aside={search.source ? <button type="button" onClick={() => set({ source: '', page: 1 })}>All</button> : `${sources.length}`}>Sources</SectionRule>
      <div className="grid max-h-72 overflow-y-auto pt-1">{sources.map(source => <button type="button" key={source} onClick={() => set({ source, page: 1 })} className={`flex h-7 items-center text-left text-sm ${search.source === source ? 'font-semibold' : search.source ? 'text-muted-foreground' : ''} hover:underline`}>{source}</button>)}</div>
    </div>
  </aside>
}

function ScoreRange({ min, max }: { min: string; max: string }) {
  const lo = Math.max(0, Math.min(1, Number(min) || 0)), hi = Math.max(lo, Math.min(1, max === '' ? 1 : Number(max)))
  return <span className="relative block h-1 rounded-full bg-rule"><i className="absolute inset-y-0 rounded-full bg-foreground" style={{ left: `${lo * 100}%`, width: `${(hi - lo) * 100}%` }} /></span>
}
