import React, { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { QueryClient, QueryClientProvider, useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { createRootRoute, createRoute, createRouter, Link, Outlet, RouterProvider, useNavigate, useSearch } from '@tanstack/react-router'
import { Archive, ArrowLeft, ChevronLeft, ChevronRight, FileText, FlaskConical, History, RotateCcw, Search, ShieldCheck, TextSearch } from 'lucide-react'
import { api, corpusParams, type CorpusFilters, type Document, type Question } from './api'
import { Classify, classifierOptions } from './classify'
import { characters, number, tail } from './format'
import { classificationBadges, hasScores } from './policy'
import { Runs } from './runs'
import { DocumentLink, Empty, ErrorBox, PageHeader, Pulse } from './ui'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { TooltipProvider } from '@/components/ui/tooltip'
import './styles.css'

const qc = new QueryClient({ defaultOptions: { queries: { staleTime: 8_000, retry: 1 } } })

function Shell() {
  const system = useQuery({ queryKey: ['system'], queryFn: api.system, staleTime: 60_000 })
  // The rail says when a run is going, from any page.
  const recent = useQuery({
    queryKey: ['runs', 'rail'],
    queryFn: () => api.runs(1, 10),
    refetchInterval: query => query.state.data?.runs.some(run => run.status === 'running') ? 2000 : 15000,
  })
  const running = recent.data?.runs.filter(run => run.status === 'running') || []
  const serverVersion = system.data?.version
  const stale = Boolean(serverVersion && serverVersion !== __CENTINEL_VERSION__)
  return <TooltipProvider delayDuration={250}><div className="shell"><aside className="rail">
    <div className="brand"><span className="brand-mark">C</span><div><b>Centinel</b><small>Corpus workspace · v{__CENTINEL_VERSION__}</small></div></div>
    {stale && <div className="rail-note rail-warning"><ShieldCheck /><div><b>Server is v{serverVersion}</b><span>This page is v{__CENTINEL_VERSION__}. Stop the old `centinel web` and start it again, then reload.</span></div></div>}
    <nav>
      <Link to="/" search={{ text: '', address: '', page: 1, source: '', usage: 'all', classifier: '', minScore: '0.5', maxScore: '' }} activeOptions={{ exact: true }}><Archive />Corpus</Link>
      <Link to="/classifiers"><FlaskConical />Classify</Link>
      <Link to="/runs" search={{ run: running[0]?.id || '', page: 1, outcome: '' }}><History />Runs{running.length > 0 && <span className="rail-live"><Pulse />{running.length} running</span>}</Link>
    </nav>
    <div className="rail-note"><ShieldCheck /><div><b>Archive stays intact</b><span>Classification changes corpus usage. Collected bytes and the log do not change.</span></div></div>
  </aside><main><Outlet /></main></div></TooltipProvider>
}

const rootRoute = createRootRoute({ component: Shell })
const corpusRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/',
  validateSearch: (search: Record<string, unknown>) => ({
    text: String(search.text || ''), address: String(search.address || ''),
    page: Math.max(1, Number(search.page || 1)), source: String(search.source || ''),
    usage: String(search.usage || 'all'), classifier: String(search.classifier || ''),
    minScore: String(search.minScore || '0.5'), maxScore: String(search.maxScore || ''),
  }),
  component: Corpus,
})
const documentRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/document/$sha',
  validateSearch: (search: Record<string, unknown>) => ({ source: String(search.source || ''), resource: String(search.resource || '') }),
  component: Reader,
})
const classifierRoute = createRoute({ getParentRoute: () => rootRoute, path: '/classifiers', component: Classify })
const runsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/runs',
  validateSearch: (search: Record<string, unknown>) => ({
    run: String(search.run || ''),
    page: Math.max(1, Number(search.page || 1)),
    outcome: String(search.outcome || ''),
  }),
  component: Runs,
})
const router = createRouter({ basepath: '/web', routeTree: rootRoute.addChildren([corpusRoute, documentRoute, classifierRoute, runsRoute]) })
declare module '@tanstack/react-router' { interface Register { router: typeof router } }

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
      <div className="header-stat"><span>Pending classification</span><b>{query.data ? number(query.data.pending) : '—'}</b></div>
    </PageHeader>
    <form className="filter-deck" onSubmit={submit}>
      <label className="filter-field grow"><span><TextSearch />Full-text search</span><Input value={draftText} onChange={event => setDraftText(event.target.value)} placeholder="Words inside extracted documents" /></label>
      <label className="filter-field grow"><span><Search />Address or title</span><Input value={draftAddress} onChange={event => setDraftAddress(event.target.value)} placeholder="tampa.gov/agenda.pdf" /></label>
      <FilterSelect label="Source" value={search.source || 'all'} onChange={value => set({ source: value === 'all' ? '' : value, page: 1 })} options={[['all', 'All sources'], ...(query.data?.sources || []).map(value => [value, value] as [string, string])]} />
      <FilterSelect label="Usage" value={search.usage} onChange={value => set({ usage: value, page: 1 })} options={usageOptions} />
      <FilterSelect label="Classifier" value={search.classifier || 'all'} onChange={value => set({ classifier: value === 'all' ? '' : value, page: 1 })} options={[['all', 'All classifiers'], ...classifierOptions(saved)]} />
      {search.classifier && <label className="filter-field score-filter"><span>Score from</span><Input aria-label="Minimum classifier score" type="number" min="0" max="1" step="0.05" value={search.minScore} onChange={event => set({ minScore: event.target.value, page: 1 })} /></label>}
      {search.classifier && <label className="filter-field score-filter"><span>Score to</span><Input aria-label="Maximum classifier score" type="number" min="0" max="1" step="0.05" value={search.maxScore} placeholder="1" onChange={event => set({ maxScore: event.target.value, page: 1 })} /></label>}
      <Button type="submit"><Search />Apply</Button>
    </form>
    {query.error ? <ErrorBox error={query.error} /> : <section className="panel corpus-panel">
      <div className="panel-top"><span>{query.data ? `${number(query.data.total)} documents · ${characters(query.data.total_chars)}` : 'Reading index…'}</span><small>Latest indexed derivation per Resource</small></div>
      <div className="table-wrap"><table className="corpus-table"><thead><tr><th>Document</th><th>Source</th><th>Scale</th><th>Classification</th><th>Usage</th></tr></thead><tbody>
        {query.data?.documents.map(doc => <DocumentRow key={`${doc.source}:${doc.resource}:${doc.derived_sha}`} doc={doc} questions={saved} />)}
      </tbody></table></div>
      {query.data && !query.data.documents.length && <Empty>No documents match these filters. Clear one filter and try again.</Empty>}
      {query.data && <div className="pager"><Button variant="secondary" size="sm" disabled={search.page === 1} onClick={() => set({ page: search.page - 1 })}><ChevronLeft />Previous</Button><span>Page {search.page} of {pages}</span><Button variant="secondary" size="sm" disabled={search.page >= pages} onClick={() => set({ page: search.page + 1 })}>Next<ChevronRight /></Button></div>}
    </section>}
  </>
}

function FilterSelect({ label, value, onChange, options }: { label: string; value: string; onChange: (value: string) => void; options: Array<[string, string]> }) {
  return <label className="filter-field compact"><span>{label}</span><Select value={value} onValueChange={onChange}><SelectTrigger aria-label={label}><SelectValue /></SelectTrigger><SelectContent>{options.map(([key, text]) => <SelectItem value={key} key={key}>{text}</SelectItem>)}</SelectContent></Select></label>
}
const usageOptions: Array<[string, string]> = [['all', 'All usage'], ['included', 'Included'], ['excluded', 'Excluded'], ['pending', 'Pending']]

function DocumentRow({ doc, questions }: { doc: Document; questions: Question[] }) {
  const badges = classificationBadges(doc.classifications, questions).slice(0, 4)
  return <tr><td><DocumentLink doc={doc}><span className="doc-icon"><FileText /></span><span><b>{doc.title || tail(doc.resource)}</b><small>{doc.resource}</small></span></DocumentLink></td>
    <td><Badge variant="muted">{doc.source}</Badge></td><td>{characters(doc.chars)}<small>{number(doc.chunks)} chunks</small></td>
    <td>{badges.length ? badges.map(badge => <Badge key={badge.key} variant={badge.tone}>{badge.text}</Badge>) : <span className="muted">{hasScores(doc.classifications) ? 'No tags' : 'Pending'}</span>}</td>
    <td><span className={`status ${doc.excluded ? 'excluded' : 'included'}`}>{doc.excluded ? 'Excluded' : 'Included'}</span></td></tr>
}

function Reader() {
  const { sha } = documentRoute.useParams()
  const search = documentRoute.useSearch()
  const doc = { derived_sha: sha, source: search.source, resource: search.resource }
  const query = useQuery({ queryKey: ['read', doc], queryFn: () => api.read(doc) })
  const client = useQueryClient()
  const restore = useMutation({ mutationFn: () => api.restore(doc), onSuccess: () => client.invalidateQueries({ queryKey: ['corpus'] }) })
  return <>
    <PageHeader eyebrow={query.data?.source || search.source || 'Document'} title={query.data ? tail(query.data.url) : tail(search.resource || sha)}>
      <Button variant="secondary" onClick={() => history.back()}><ArrowLeft />Back</Button>
    </PageHeader>
    {query.error ? <ErrorBox error={query.error} /> : query.data ? <div className="reader-grid">
      <article className="document"><div className="document-meta"><span>{query.data.kind}</span><span>{number(query.data.total_chars)} characters</span><span>{query.data.observed_at}</span></div><pre>{query.data.text}</pre></article>
      <aside className="inspector"><h2>Provenance</h2><dl><dt>Resource</dt><dd>{query.data.url}</dd><dt>Blob SHA</dt><dd className="mono">{query.data.blob_sha}</dd><dt>Text SHA</dt><dd className="mono">{query.data.derived_sha}</dd><dt>Extractor</dt><dd>{query.data.tool}</dd></dl><Button variant="secondary" className="wide" disabled={restore.isPending} onClick={() => restore.mutate()}><RotateCcw />Restore usage</Button>{restore.isSuccess && <p className="success-note">Restored to corpus usage.</p>}{restore.error && <ErrorBox error={restore.error} />}</aside>
    </div> : <div className="loading">Reading the complete document…</div>}
  </>
}

createRoot(document.getElementById('root')!).render(<React.StrictMode><QueryClientProvider client={qc}><RouterProvider router={router} /></QueryClientProvider></React.StrictMode>)
