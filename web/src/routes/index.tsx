import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import React, { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { createFileRoute, useNavigate, useSearch } from '@tanstack/react-router'
import { ChevronLeft, ChevronRight, FileText, Search, TextSearch } from 'lucide-react'
import { api, corpusParams, type CorpusFilters, type Document, type Question } from '../api'
import { classifierOptions } from '../classify'
import { characters, number, tail } from '../format'
import { classificationBadges, hasScores } from '../policy'
import { DocumentLink, Empty, ErrorBox, PageHeader } from '../ui'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'

export const Route = createFileRoute('/')({
  validateSearch: (search: Record<string, unknown>) => ({
    text: String(search.text || ''), address: String(search.address || ''),
    page: Math.max(1, Number(search.page || 1)), source: String(search.source || ''),
    usage: String(search.usage || 'all'), classifier: String(search.classifier || ''),
    minScore: String(search.minScore || '0.5'), maxScore: String(search.maxScore || ''),
  }),
  component: Corpus,
})

function Corpus() {
  const search = useSearch({ from: '/' })
  const navigate = useNavigate({ from: '/' })
  const [draftText, setDraftText] = useState(search.text)
  const [draftAddress, setDraftAddress] = useState(search.address)
  const filters: CorpusFilters = { search: search.text, address: search.address, source: search.source, usage: search.usage, classifier: search.classifier, min_score: search.minScore, max_score: search.maxScore }
  const params = corpusParams(filters, search.page, 25)
  const query = useQuery({ queryKey: ['corpus', params.toString()], queryFn: () => api.corpus(params) })
  const questions = useQuery({ queryKey: ['questions'], queryFn: api.questions })
  const set = (next: Partial<typeof search>) => navigate({ search: previous => ({ ...previous, ...next }) })
  const submit = (event: React.FormEvent) => { event.preventDefault(); set({ text: draftText.trim(), address: draftAddress.trim(), page: 1 }) }
  const pages = Math.max(1, Math.ceil((query.data?.total || 0) / (query.data?.page_size || 25)))
  const saved = questions.data?.questions || []

  return <>
    <PageHeader eyebrow="Disposable index · evidentiary archive" title="Corpus" detail="Browse every indexed document, or search the extracted text.">
      <div className="grid gap-1 border-l pl-4 text-sm [&_span]:text-muted-foreground [&_b]:text-2xl"><span>Pending classification</span><b>{query.data ? number(query.data.pending) : '—'}</b></div>
    </PageHeader>
    <form className="flex flex-wrap items-end gap-3 rounded-t-lg border bg-muted p-4" onSubmit={submit}>
      <label className="grid min-w-32 gap-2 text-xs font-medium [&_span]:flex [&_span]:items-center [&_span]:gap-1 [&_svg]:size-4 [&_[data-slot=select-trigger]]:w-full flex-1"><span><TextSearch />Full-text search</span><Input value={draftText} onChange={event => setDraftText(event.target.value)} placeholder="Words inside extracted documents" /></label>
      <label className="grid min-w-32 gap-2 text-xs font-medium [&_span]:flex [&_span]:items-center [&_span]:gap-1 [&_svg]:size-4 [&_[data-slot=select-trigger]]:w-full flex-1"><span><Search />Address or title</span><Input value={draftAddress} onChange={event => setDraftAddress(event.target.value)} placeholder="tampa.gov/agenda.pdf" /></label>
      <FilterSelect label="Source" value={search.source || 'all'} onChange={value => set({ source: value === 'all' ? '' : value, page: 1 })} options={[['all', 'All sources'], ...(query.data?.sources || []).map(value => [value, value] as [string, string])]} />
      <FilterSelect label="Usage" value={search.usage} onChange={value => set({ usage: value, page: 1 })} options={usageOptions} />
      <FilterSelect label="Classifier" value={search.classifier || 'all'} onChange={value => set({ classifier: value === 'all' ? '' : value, page: 1 })} options={[['all', 'All classifiers'], ...classifierOptions(saved)]} />
      {search.classifier && <label className="grid min-w-32 gap-2 text-xs font-medium [&_span]:flex [&_span]:items-center [&_span]:gap-1 [&_svg]:size-4 [&_[data-slot=select-trigger]]:w-full w-32"><span>Score from</span><Input aria-label="Minimum classifier score" type="number" min="0" max="1" step="0.05" value={search.minScore} onChange={event => set({ minScore: event.target.value, page: 1 })} /></label>}
      {search.classifier && <label className="grid min-w-32 gap-2 text-xs font-medium [&_span]:flex [&_span]:items-center [&_span]:gap-1 [&_svg]:size-4 [&_[data-slot=select-trigger]]:w-full w-32"><span>Score to</span><Input aria-label="Maximum classifier score" type="number" min="0" max="1" step="0.05" value={search.maxScore} placeholder="1" onChange={event => set({ maxScore: event.target.value, page: 1 })} /></label>}
      <Button type="submit"><Search />Apply</Button>
    </form>
    {query.error ? <ErrorBox error={query.error} /> : <section className="rounded-lg border bg-card text-card-foreground shadow-sm rounded-t-none border-t-0">
      <div className="flex min-h-11 items-center justify-between border-b px-4 text-sm font-medium [&_small]:text-muted-foreground"><span>{query.data ? `${number(query.data.total)} documents · ${characters(query.data.total_chars)}` : 'Reading index…'}</span><small>Latest indexed derivation per Resource</small></div>
      <div className="overflow-x-auto"><Table className="min-w-[760px] [&_th:first-child]:w-[43%]"><TableHeader><TableRow><TableHead>Document</TableHead><TableHead>Source</TableHead><TableHead>Scale</TableHead><TableHead>Classification</TableHead><TableHead>Usage</TableHead></TableRow></TableHeader><TableBody>
        {query.data?.documents.map(doc => <DocumentRow key={`${doc.source}:${doc.resource}:${doc.derived_sha}`} doc={doc} questions={saved} />)}
      </TableBody></Table></div>
      {query.data && !query.data.documents.length && <Empty>No documents match these filters. Clear one filter and try again.</Empty>}
      {query.data && <div className="flex h-14 items-center justify-end gap-3 border-t px-4 text-xs"><Button variant="secondary" size="sm" disabled={search.page === 1} onClick={() => set({ page: search.page - 1 })}><ChevronLeft />Previous</Button><span>Page {search.page} of {pages}</span><Button variant="secondary" size="sm" disabled={search.page >= pages} onClick={() => set({ page: search.page + 1 })}>Next<ChevronRight /></Button></div>}
    </section>}
  </>
}

function FilterSelect({ label, value, onChange, options }: { label: string; value: string; onChange: (value: string) => void; options: Array<[string, string]> }) {
  return <label className="grid min-w-32 gap-2 text-xs font-medium [&_span]:flex [&_span]:items-center [&_span]:gap-1 [&_svg]:size-4 [&_[data-slot=select-trigger]]:w-full min-w-32"><span>{label}</span><Select value={value} onValueChange={onChange}><SelectTrigger aria-label={label}><SelectValue /></SelectTrigger><SelectContent>{options.map(([key, text]) => <SelectItem value={key} key={key}>{text}</SelectItem>)}</SelectContent></Select></label>
}
const usageOptions: Array<[string, string]> = [['all', 'All usage'], ['included', 'Included'], ['excluded', 'Excluded'], ['pending', 'Pending']]

function DocumentRow({ doc, questions }: { doc: Document; questions: Question[] }) {
  const badges = classificationBadges(doc.classifications, questions).slice(0, 4)
  return <TableRow><TableCell className="whitespace-normal"><DocumentLink doc={doc} className="flex items-center gap-3"><span className="grid size-8 shrink-0 place-items-center rounded bg-muted [&_svg]:size-4"><FileText /></span><span className="grid min-w-0"><b>{doc.title || tail(doc.resource)}</b><small className="break-all text-muted-foreground">{doc.resource}</small></span></DocumentLink></TableCell>
    <TableCell><Badge variant="secondary">{doc.source}</Badge></TableCell><TableCell><span className="grid">{characters(doc.chars)}<small className="text-muted-foreground">{number(doc.chunks)} chunks</small></span></TableCell>
    <TableCell>{badges.length ? badges.map(badge => <Badge key={badge.key} variant={badge.tone === 'warning' ? 'outline' : badge.tone === 'muted' ? 'secondary' : 'default'}>{badge.text}</Badge>) : <span className="text-muted-foreground">{hasScores(doc.classifications) ? 'No tags' : 'Pending'}</span>}</TableCell>
    <TableCell><span className="inline-flex items-center gap-1 text-xs font-medium">{doc.excluded ? 'Excluded' : 'Included'}</span></TableCell></TableRow>
}

