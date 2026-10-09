import React, { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { createFileRoute, useNavigate, useSearch } from '@tanstack/react-router'
import { ArrowUp, ChevronDown, ChevronLeft, ChevronRight, X } from 'lucide-react'
import type { CorpusFacets, CorpusFilters, Document, Question } from '../api'
import { classifierOptions } from '../classify'
import { Candle, useElapsed, useShownInPlace } from '../feedback'
import { characters, compact, number, tail } from '../format'
import { classificationBadges, hasScores } from '../policy'
import { queries } from '../queries'
import { DocumentLink, documentTransition, Empty, ErrorBox, shortSha } from '../ui'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { Skeleton } from '@/components/ui/skeleton'

export const Route = createFileRoute('/')({
  validateSearch: (search: Record<string, unknown>) => ({
    text: String(search.text || ''), address: String(search.address || ''),
    page: Math.max(1, Number(search.page || 1)), source: String(search.source || ''),
    usage: String(search.usage || 'all'), classifier: String(search.classifier || ''),
    minScore: String(search.minScore || '0.5'), maxScore: String(search.maxScore || ''),
  }),
  loaderDeps: ({ search }) => search,
  // A search can take half a minute, so the page never waits on one: the box stays, and
  // the results wait in place. Only the opening counts are worth holding the page for.
  loader: ({ context: { queryClient }, deps }) => {
    void queryClient.prefetchQuery(queries.questions())
    if (asking(deps)) void queryClient.prefetchQuery(queries.corpus(filtersOf(deps), deps.page))
    else return queryClient.ensureQueryData(everything)
  },
  pendingComponent: SearchSkeleton,
  component: SearchPage,
})

type Search = ReturnType<typeof Route.useSearch>
type Set = (next: Partial<Search>) => void

/** The page's search params as the corpus filter. One mapping, for the loader and the page. */
const filtersOf = (search: Search): CorpusFilters => ({ search: search.text, address: search.address, source: search.source, usage: search.usage, classifier: search.classifier, min_score: search.minScore, max_score: search.maxScore })
/** Whether anything has been asked. Until then the page is a question, not a list. */
const asking = (search: Search) => Boolean(search.text || search.address || search.source || search.classifier || search.usage !== 'all')
/** The whole corpus in one row: its counts and facets, for the opening page and the filters. */
const everything = queries.corpus({}, 1, 1)
const sourcesIn = (value: string) => value.split(',').map(source => source.trim()).filter(Boolean)
const cleared: Search = { text: '', address: '', page: 1, source: '', usage: 'all', classifier: '', minScore: '0.5', maxScore: '' }

/** One colour per kind of filter, so a chip says what it narrows by. */
const tone = {
  classifier: { on: 'bg-flame-soft text-flame-ink shadow-[inset_0_0_0_1px_#F0D6B0]', dot: 'bg-flame', bar: 'bg-flame', faint: 'bg-[#EBD3B4]', label: 'text-flame-ink' },
  source: { on: 'bg-slate-soft text-slate shadow-[inset_0_0_0_1px_#C9D6E2]', dot: 'bg-slate', bar: 'bg-slate', faint: 'bg-[#C9D6E2]', label: 'text-slate' },
  usage: { on: 'bg-moss-soft text-moss shadow-[inset_0_0_0_1px_#C8DBC3]', dot: 'bg-moss', bar: 'bg-moss', faint: 'bg-[#C8DBC3]', label: 'text-moss' },
  words: { on: 'bg-parchment text-foreground shadow-[inset_0_0_0_1px_var(--rule)]', dot: 'bg-foreground', bar: 'bg-foreground', faint: 'bg-rule', label: 'text-foreground' },
} as const
type Tone = keyof typeof tone

function SearchPage() {
  const search = useSearch({ from: '/' })
  const navigate = useNavigate({ from: '/' })
  const set: Set = next => navigate({ search: previous => ({ ...previous, ...next }) })
  const open = asking(search)
  const query = useQuery(open ? queries.corpus(filtersOf(search), search.page) : everything)
  const whole = useQuery(everything)
  const questions = useQuery(queries.questions())
  const saved = questions.data?.questions || []
  // Before a search, filters wait in the box and go with the words. After, they refine the results at once.
  const [staged, setStaged] = useState<Search>(cleared)
  const stage: Set = next => setStaged((previous: Search) => ({ ...previous, ...next }))
  const filtersFor = (current: Search, change: Set) => <FilterBar search={current} set={change} classifiers={classifierOptions(saved)} sources={whole.data?.sources || []} facets={query.data?.facets} />

  if (!open) return <div className="flex min-h-[calc(100svh-8rem)] flex-col items-center justify-center gap-7 pb-16">
    <div className="grid justify-items-center gap-2 text-center">
      <h1 className="font-serif text-[52px] leading-[56px] tracking-[-0.01em]">What should we look for?</h1>
      <p className="text-[15px] text-muted-foreground">Search the words inside every document Centinel has collected.</p>
    </div>
    <SearchBox key="opening" initial="" large ready={asking(staged)} onSearch={text => set({ ...staged, text, page: 1 })}>{filtersFor(staged, stage)}</SearchBox>
    {whole.data && <dl className="mt-6 flex flex-wrap justify-center gap-x-12 gap-y-4 text-center">
      <Stat value={number(whole.data.total)} label="documents" />
      <Stat value={big(whole.data.total_chars)} label="characters of text" />
      <Stat value={number(whole.data.sources.length)} label="sources" />
      <Stat value={number(whole.data.pending)} label="waiting on a classifier" flame />
    </dl>}
    {whole.error && <ErrorBox error={whole.error} />}
  </div>

  const data = query.data
  // Fetching with the last results still up, or with none yet: either way, a search is under way.
  const searching = query.isFetching && (query.isPlaceholderData || !data)
  const pages = Math.max(1, Math.ceil((data?.total || 0) / (data?.page_size || 25)))
  return <>
    <div className="mb-3 flex items-center gap-3">
      <button type="button" onClick={() => navigate({ search: cleared })} className="inline-flex h-12 shrink-0 items-center gap-1 self-start text-[13px] text-muted-foreground hover:text-foreground" aria-label="New search"><ChevronLeft className="size-4" /><span className="hidden sm:inline">New</span></button>
      <SearchBox key={search.text} initial={search.text} ready busy={searching} onSearch={text => set({ text, page: 1 })}>{filtersFor(search, set)}</SearchBox>
    </div>
    <p className="flex h-5 items-center gap-2 pl-12 text-[13px] text-muted-foreground">{searching ? <Searching /> : data ? `${number(data.total)} documents · ${characters(data.total_chars)}` : ''}</p>
    <section aria-busy={searching} className={`mt-6 transition-opacity duration-(--motion-indicator) ${searching && data ? 'pointer-events-none opacity-45' : ''}`}>
      <div className="flex h-8 items-center gap-6 border-b border-foreground text-[11px] font-semibold uppercase tracking-[0.08em] text-muted-foreground">
        <span className="flex-1">Document</span><span className="hidden w-56 md:block">Scores</span><span className="w-16 text-right">Blob</span>
      </div>
      {query.error ? <ErrorBox error={query.error} /> : !data ? Array.from({ length: 8 }, (_, i) => <ResultRow key={i} questions={[]} />) : data.documents.map(doc => <ResultRow key={`${doc.source}:${doc.resource}:${doc.derived_sha}`} doc={doc} questions={saved} />)}
      {data && !searching && !data.documents.length && <Empty>No documents match. Loosen a filter or try other words.</Empty>}
      {data && data.total > 0 && <div className="flex items-center justify-between py-4 text-[13px] text-muted-foreground">
        <span>Page {search.page} of {number(pages)}</span>
        <span className="flex gap-2"><Button variant="outline" size="sm" disabled={search.page === 1} onClick={() => set({ page: search.page - 1 })}><ChevronLeft />Previous</Button><Button variant="outline" size="sm" disabled={search.page >= pages} onClick={() => set({ page: search.page + 1 })}>Next<ChevronRight /></Button></span>
      </div>}
    </section>
  </>
}

/** The count line while a search runs: a flame, and the seconds, so a slow one still reads as work. */
function Searching() {
  const seconds = useElapsed(true)
  useShownInPlace()
  return <><Candle /><span className="text-flame-ink">Searching every document{seconds >= 1 ? ` · ${seconds}s` : '…'}</span>{seconds >= 8 && <span>Long searches across the whole archive can take a minute.</span>}</>
}

/** A large count at a glance: 1.35B, 312M, 48K. */
const big = (n: number) => n >= 1e9 ? `${(n / 1e9).toFixed(2)}B` : n >= 1e6 ? `${Math.round(n / 1e6)}M` : n >= 1e4 ? `${Math.round(n / 1e3)}K` : number(n)

function Stat({ value, label, flame }: { value: string; label: string; flame?: boolean }) {
  return <div className="flex flex-col-reverse gap-0.5"><dt className="text-xs text-muted-foreground">{label}</dt><dd className={`text-[26px] leading-8 font-semibold tracking-[-0.02em] ${flame ? 'text-flame-ink' : ''}`}>{value}</dd></div>
}

/**
 * The question box: what to look for on top, the filters and the send button along its
 * foot, one surface. Large on the opening page, compact above the results.
 */
function SearchBox({ initial, large, ready, busy, onSearch, children }: { initial: string; large?: boolean; ready?: boolean; busy?: boolean; onSearch: (text: string) => void; children: React.ReactNode }) {
  const [draft, setDraft] = useState(initial)
  return <form className={`w-full ${large ? 'max-w-[760px]' : 'flex-1'}`} onSubmit={event => { event.preventDefault(); if (draft.trim() || ready) onSearch(draft.trim()) }}>
    <div className="grid rounded-[22px] border border-input bg-background shadow-[0_1px_2px_rgba(26,23,18,0.04),0_10px_30px_rgba(26,23,18,0.06)] focus-within:border-[#CFC6B5]">
      <input autoFocus={large} value={draft} onChange={event => setDraft(event.target.value)} placeholder="Search the corpus: a phrase, a name, a project" aria-label="Search the corpus"
        className={`bg-transparent px-5 outline-none placeholder:text-muted-foreground ${large ? 'h-[72px] text-[18px]' : 'h-14 text-[15px]'}`} />
      <div className="flex min-w-0 items-center gap-2 px-3 pb-3">
        {children}
        <button type="submit" disabled={!draft.trim() && !ready} aria-label={busy ? 'Searching' : 'Search'} className="relative ml-auto grid size-10 shrink-0 place-items-center rounded-full bg-foreground text-parchment transition-opacity duration-(--motion-micro) disabled:opacity-25">
          {busy ? <><span aria-hidden className="absolute -inset-1 animate-spin rounded-full border-2 border-transparent border-t-flame motion-reduce:animate-none" /><Candle /></> : <ArrowUp className="size-[18px]" />}
        </button>
      </div>
    </div>
  </form>
}

/** The filters along the foot of the box: each opens its own panel and wears its own colour once set. */
function FilterBar({ search, set, classifiers, sources, facets }: { search: Search; set: Set; classifiers: Array<[string, string]>; sources: string[]; facets?: CorpusFacets }) {
  const chosen = sourcesIn(search.source)
  const classifier = classifiers.find(([key]) => key === search.classifier)?.[1] || search.classifier
  const any = Boolean(search.classifier || chosen.length || search.usage !== 'all' || search.address)
  return <div className="flex min-w-0 flex-1 flex-nowrap items-center gap-1">
    <Filter kind="classifier" label="Classifiers" value={search.classifier ? `${classifier} ${search.minScore || '0'}–${search.maxScore || '1'}` : ''} onClear={() => set({ classifier: '', page: 1 })}>
      <ClassifierPanel search={search} set={set} classifiers={classifiers} facets={facets} />
    </Filter>
    <Divider />
    <Filter kind="source" label="Sources" value={chosen.length ? `${chosen[0]}${chosen.length > 1 ? ` +${chosen.length - 1}` : ''}` : ''} onClear={() => set({ source: '', page: 1 })}>
      <SourcePanel chosen={chosen} set={set} sources={sources} counts={facets?.sources} />
    </Filter>
    <Divider />
    <Filter kind="usage" label="Usage" value={search.usage !== 'all' ? search.usage : ''} onClear={() => set({ usage: 'all', page: 1 })}>
      <UsagePanel usage={search.usage} set={set} counts={facets?.usage} />
    </Filter>
    <Divider />
    <Filter kind="words" label="Address" value={search.address} onClear={() => set({ address: '', page: 1 })}>
      <AddressPanel address={search.address} set={set} />
    </Filter>
    {any && <button type="button" onClick={() => set({ classifier: '', source: '', usage: 'all', address: '', page: 1 })} className="ml-1 h-8 shrink-0 px-2 text-[13px] text-muted-foreground hover:text-foreground">Clear</button>}
  </div>
}

const Divider = () => <span aria-hidden className="mx-0.5 h-5 w-px shrink-0 bg-rule" />

function Filter({ kind, label, value, onClear, children }: { kind: Tone; label: string; value: string; onClear: () => void; children: React.ReactNode }) {
  return <Popover>
    <span className={`inline-flex h-9 min-w-0 items-center rounded-full text-[14px] transition-colors duration-(--motion-micro) ${value.length > 14 ? 'shrink' : 'shrink-0'} ${value ? tone[kind].on : 'text-muted-foreground hover:bg-[#FBF8F1] hover:text-foreground'}`}>
      <PopoverTrigger type="button" className="inline-flex h-full min-w-0 items-center gap-2 rounded-full pr-2 pl-3 outline-none focus-visible:ring-2 focus-visible:ring-flame/40">
        <span className={`size-1.5 shrink-0 rounded-full ${tone[kind].dot}`} />
        {value ? <b className="min-w-0 truncate font-semibold" title={value}>{value}</b> : <span>{label}</span>}
        {!value && <ChevronDown className="size-3.5 opacity-60" />}
      </PopoverTrigger>
      {value && <button type="button" aria-label={`Clear ${label}`} onClick={onClear} className="shrink-0 pr-2.5 opacity-60 hover:opacity-100"><X className="size-3.5" /></button>}
    </span>
    <PopoverContent align="start" sideOffset={10} className="w-80 p-3">{children}</PopoverContent>
  </Popover>
}

function PanelTitle({ kind, children }: { kind: Tone; children: React.ReactNode }) {
  return <span className="mb-1 inline-flex items-center gap-2 px-1 text-[11px] font-semibold uppercase tracking-[0.08em] text-muted-foreground"><span className={`size-1.5 rounded-full ${tone[kind].dot}`} />{children}</span>
}

function ClassifierPanel({ search, set, classifiers, facets }: { search: Search; set: Set; classifiers: Array<[string, string]>; facets?: CorpusFacets }) {
  return <div className="grid gap-1">
    <PanelTitle kind="classifier">Classifiers · score range</PanelTitle>
    <div className="grid max-h-96 gap-1 overflow-y-auto">
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
      {!classifiers.length && <p className="px-2 py-2 text-sm text-muted-foreground">No classifiers yet. Write one on Classify.</p>}
    </div>
  </div>
}

function SourcePanel({ chosen, set, sources, counts }: { chosen: string[]; set: Set; sources: string[]; counts?: Record<string, number> }) {
  const max = Math.max(1, ...Object.values(counts || {}))
  const toggle = (source: string) => set({ source: (chosen.includes(source) ? chosen.filter(s => s !== source) : [...chosen, source]).join(','), page: 1 })
  return <div className="grid gap-1">
    <PanelTitle kind="source">Sources · pick any</PanelTitle>
    <div className="grid max-h-96 overflow-y-auto">
      {sources.map(source => {
        const active = chosen.includes(source)
        const count = counts?.[source]
        return <button type="button" key={source} aria-pressed={active} onClick={() => toggle(source)} className={`grid gap-1 rounded-lg px-2 py-1.5 text-left ${active ? 'bg-slate-soft' : 'hover:bg-[#FBF9F4]'}`}>
          <span className="flex items-center justify-between gap-2 text-sm">
            <span className="flex min-w-0 items-center gap-2"><span className={`grid size-3.5 shrink-0 place-items-center rounded-[4px] ${active ? 'bg-slate text-white' : 'shadow-[inset_0_0_0_1.5px_#B9C6D3]'}`}>{active && <svg viewBox="0 0 24 24" className="size-2.5" fill="none" stroke="currentColor" strokeWidth="4"><path d="M5 12l5 5L20 7" /></svg>}</span><span className={`truncate ${active ? `font-semibold ${tone.source.label}` : ''}`}>{source}</span></span>
            {count != null && <span className="font-mono text-xs text-muted-foreground">{number(count)}</span>}
          </span>
          {count != null && <Meter value={count} max={max} kind="source" strong={active} />}
        </button>
      })}
    </div>
  </div>
}

const usageOptions: Array<[string, string]> = [['all', 'All'], ['included', 'Included'], ['excluded', 'Excluded'], ['pending', 'Pending']]

function UsagePanel({ usage, set, counts }: { usage: string; set: Set; counts?: Record<string, number> }) {
  const all = counts ? (counts.included || 0) + (counts.excluded || 0) : 0
  return <div className="grid gap-1">
    <PanelTitle kind="usage">Usage</PanelTitle>
    {usageOptions.map(([value, label]) => {
      const count = !counts ? undefined : value === 'all' ? all : counts[value]
      const active = usage === value
      return <button type="button" key={value} onClick={() => set({ usage: value, page: 1 })} className={`grid gap-1 rounded-lg px-2 py-1.5 text-left ${active ? 'bg-moss-soft' : 'hover:bg-[#FBF9F4]'}`}>
        <span className="flex justify-between text-sm"><span className={active ? `font-semibold ${tone.usage.label}` : ''}>{label}</span>{count != null && <span className="font-mono text-xs text-muted-foreground">{number(count)}</span>}</span>
        {count != null && <Meter value={count} max={Math.max(1, all)} kind="usage" strong={active} />}
      </button>
    })}
  </div>
}

function AddressPanel({ address, set }: { address: string; set: Set }) {
  const [draft, setDraft] = useState(address)
  // Its own form, inside the search box's. React carries a submit up through the portal, so it stops here.
  return <form className="grid gap-2" onSubmit={event => { event.preventDefault(); event.stopPropagation(); set({ address: draft.trim(), page: 1 }) }}>
    <PanelTitle kind="words">Address or title contains</PanelTitle>
    <div className="flex gap-2"><Input autoFocus className="h-8 text-[13px]" value={draft} onChange={event => setDraft(event.target.value)} placeholder="tampa.gov/agenda" /><Button size="sm" type="submit">Apply</Button></div>
  </form>
}

/** One result. Without a document it is the same row with every data slot masked, so the page and its skeleton share one layout. */
function ResultRow({ doc, questions }: { doc?: Document; questions: Question[] }) {
  if (!doc) return <div aria-busy className="flex items-start gap-6 border-b py-4">
    <span className="grid min-w-0 flex-1 gap-1.5">
      <b className="text-[15px] leading-5 font-semibold"><Skeleton mask="City Council Regular Session — Minutes" /></b>
      <span className="text-xs"><Skeleton mask="tampa.gov · https://www.tampa.gov/agendas/2025/res.pdf · 14.2k chars" /></span>
    </span>
    <span className="hidden w-56 gap-1.5 pt-0.5 md:flex"><Skeleton className="h-5 w-20" /><Skeleton className="h-5 w-24" /></span>
    <span className="w-16 pt-0.5 text-right font-mono text-xs"><Skeleton mask="a3f91c2" /></span>
  </div>
  const badges = classificationBadges(doc.classifications, questions).slice(0, 3)
  return <div className={`group flex items-start gap-6 border-b py-4 ${doc.excluded ? 'opacity-55' : ''}`}>
    <DocumentLink doc={doc} className="grid min-w-0 flex-1 gap-1.5">
      <b className="w-fit text-[15px] leading-5 font-semibold group-hover:underline" style={documentTransition(doc)}>{doc.title || tail(doc.resource)}</b>
      <span className="flex min-w-0 items-center gap-2 text-xs text-muted-foreground">
        <span className={`shrink-0 rounded px-1.5 py-px font-medium ${tone.source.on}`}>{doc.source}</span>
        <span className="truncate">{doc.resource}</span>
        <span className="shrink-0">· {characters(doc.chars)}</span>
        {doc.excluded && <span className="shrink-0 rounded px-1.5 py-px font-medium text-destructive shadow-[inset_0_0_0_1px_#E2B1A8]">excluded</span>}
      </span>
    </DocumentLink>
    <span className="hidden w-56 flex-wrap gap-1.5 pt-0.5 md:flex">
      {badges.length ? badges.map(badge => <span key={badge.key} className={`rounded px-2 py-0.5 text-xs font-semibold ${badge.tone === 'muted' ? 'text-muted-foreground shadow-[inset_0_0_0_1px_var(--rule)]' : tone.classifier.on}`}>{badge.text}</span>) : <span className="text-xs text-muted-foreground">{hasScores(doc.classifications) ? 'No tags' : 'Not classified'}</span>}
    </span>
    <span className="w-16 pt-0.5 text-right font-mono text-xs text-muted-foreground">{shortSha(doc.blob_sha)}</span>
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

/** Search before its data arrives: the results page with every row masked. */
function SearchSkeleton() {
  return <div aria-busy>
    <Skeleton className="mb-3 ml-12 h-[110px] max-w-[760px] rounded-[22px]" />
    <section className="mt-6">
      <div className="flex h-8 items-center border-b border-foreground text-[11px] font-semibold uppercase tracking-[0.08em] text-muted-foreground">Document</div>
      {Array.from({ length: 8 }, (_, i) => <ResultRow key={i} questions={[]} />)}
    </section>
  </div>
}
