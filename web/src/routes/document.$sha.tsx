import { useMemo, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { createFileRoute } from '@tanstack/react-router'
import { ArrowLeft, Download, ExternalLink, RotateCcw } from 'lucide-react'
import { api, originalUrl, type ReadReport } from '../api'
import { parseCsv } from '../csv'
import { number, tail } from '../format'
import { ErrorBox, PageHeader, SectionRule, shortSha } from '../ui'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'

export const Route = createFileRoute('/document/$sha')({
  validateSearch: (search: Record<string, unknown>) => ({ source: String(search.source || ''), resource: String(search.resource || '') }),
  component: Reader,
})

/** The kinds the reader can show as collected. Everything else is read as text and downloaded. */
const originals: Record<string, string[]> = { pdf: ['Original'], csv: ['Table'], html: ['Page', 'Source'] }

function Reader() {
  const { sha } = Route.useParams()
  const search = Route.useSearch()
  const doc = { derived_sha: sha, source: search.source, resource: search.resource }
  const query = useQuery({ queryKey: ['read', doc], queryFn: () => api.read(doc) })
  const client = useQueryClient()
  const restore = useMutation({ mutationFn: () => api.restore(doc), onSuccess: () => client.invalidateQueries({ queryKey: ['corpus'] }) })
  const read = query.data
  const tabs = [...(read ? originals[read.kind] || [] : []), 'Text']
  const [tab, setTab] = useState<string>()
  const current = tab && tabs.includes(tab) ? tab : tabs[0]

  return <>
    <PageHeader
      eyebrow={<button type="button" onClick={() => history.back()} className="inline-flex items-center gap-1 hover:text-foreground"><ArrowLeft className="size-3.5" />Back</button>}
      title={read ? tail(read.url) : tail(search.resource || sha)}
    >
      {read && <div className="flex shrink-0 gap-2">
        <Button variant="outline" asChild><a href={read.url} target="_blank" rel="noreferrer">Open source<ExternalLink /></a></Button>
        <Button asChild><a href={originalUrl(read.blob_sha, read.source, true)}><Download />Download {read.kind.toUpperCase()}</a></Button>
      </div>}
    </PageHeader>
    {query.error ? <ErrorBox error={query.error} /> : read ? <>
      <div className="-mt-4 mb-5 flex flex-wrap items-center gap-2 text-[13px] text-muted-foreground">
        <span className="rounded bg-foreground px-2 py-0.5 text-[11px] font-bold tracking-[0.06em] text-parchment">{read.kind.toUpperCase()}</span>
        <span className="wrap-anywhere">{read.source} · {read.url} · {number(read.total_chars)} characters · <span className="font-mono">{shortSha(read.blob_sha)}</span></span>
      </div>
      <div className="mb-5 flex gap-6 border-b" role="tablist">
        {tabs.map(name => <button type="button" role="tab" aria-selected={current === name} key={name} onClick={() => setTab(name)} className={`-mb-px pb-2.5 text-sm ${current === name ? 'border-b-2 border-foreground font-semibold' : 'text-muted-foreground hover:text-foreground'}`}>{name === 'Text' ? `Text · ${number(read.total_chars)} chars` : name}</button>)}
      </div>
      <div className="flex flex-col gap-6 xl:flex-row xl:items-start">
        <div className="min-w-0 flex-1">
          {current === 'Original' && <iframe title="PDF" src={originalUrl(read.blob_sha, read.source)} className="h-[78vh] w-full rounded-[10px] border bg-[#3A352D]" />}
          {current === 'Page' && <iframe title="Page as collected" sandbox="" src={originalUrl(read.blob_sha, read.source)} className="h-[78vh] w-full rounded-[10px] border bg-white" />}
          {current === 'Source' && <Source read={read} />}
          {current === 'Table' && <CsvTable read={read} />}
          {current === 'Text' && <pre className="whitespace-pre-wrap wrap-anywhere rounded-[10px] border p-6 font-sans text-sm leading-7">{read.text}</pre>}
        </div>
        <aside className="grid w-full shrink-0 gap-6 xl:w-[260px]">
          <div className="grid gap-3">
            <SectionRule>Provenance</SectionRule>
            <Fact label="Blob"><span className="font-mono">{shortSha(read.blob_sha)}…{read.blob_sha.slice(-4)}</span></Fact>
            <Fact label="Text extracted by">{read.tool}</Fact>
            <Fact label="Collected">{read.observed_at}</Fact>
          </div>
          <Button variant="outline" disabled={restore.isPending} onClick={() => restore.mutate()}><RotateCcw />Restore usage</Button>
          {restore.isSuccess && <p className="text-sm">Restored to corpus usage.</p>}
          {restore.error && <ErrorBox error={restore.error} />}
        </aside>
      </div>
    </> : <div className="py-6 text-sm text-muted-foreground">Reading the complete document…</div>}
  </>
}

function Fact({ label, children }: { label: string; children: React.ReactNode }) {
  return <div className="grid gap-0.5"><span className="text-xs text-muted-foreground">{label}</span><span className="text-[13px] wrap-anywhere">{children}</span></div>
}

function useOriginalText(read: ReadReport) {
  return useQuery({
    queryKey: ['original', read.blob_sha, read.source],
    queryFn: async () => {
      const res = await fetch(originalUrl(read.blob_sha, read.source))
      if (!res.ok) throw new Error(`The original could not be read (${res.status}).`)
      return res.text()
    },
    staleTime: Infinity,
  })
}

/** The HTML as served, line by line. Shown as text; it never runs here. */
function Source({ read }: { read: ReadReport }) {
  const source = useOriginalText(read)
  if (source.error) return <ErrorBox error={source.error} />
  const lines = (source.data || '').split('\n')
  return <div className="overflow-hidden rounded-[10px] bg-[#1F1C17]">
    <div className="flex h-10 items-center px-3.5 font-mono text-xs text-[#B9AE98]">{tail(read.url)} · {number(lines.length)} lines</div>
    <div className="max-h-[72vh] overflow-auto py-3 font-mono text-[12.5px] leading-[21px]">
      {source.isPending ? <div className="px-3.5 text-[#B9AE98]">Loading…</div> : lines.map((line, i) => <div key={i} className="flex px-3.5"><span className="w-12 shrink-0 select-none text-[#6B6458]">{i + 1}</span><span className="whitespace-pre text-[#F4EEE1]">{line}</span></div>)}
    </div>
  </div>
}

const csvRowCap = 1000

function CsvTable({ read }: { read: ReadReport }) {
  const source = useOriginalText(read)
  const [filter, setFilter] = useState('')
  const rows = useMemo(() => parseCsv(source.data || ''), [source.data])
  const [header, ...body] = rows
  const needle = filter.trim().toLowerCase()
  const matching = needle ? body.filter(row => row.some(cell => cell.toLowerCase().includes(needle))) : body
  if (source.error) return <ErrorBox error={source.error} />
  return <div className="overflow-hidden rounded-[10px] border">
    <div className="flex h-11 items-center gap-3 border-b px-3.5">
      <Input className="h-8 max-w-72 text-[13px]" placeholder="Filter rows" value={filter} onChange={event => setFilter(event.target.value)} />
      <span className="text-[13px] text-muted-foreground">{source.isPending ? 'Loading…' : `${number(matching.length)} of ${number(body.length)} rows`}</span>
    </div>
    <div className="max-h-[70vh] overflow-auto">
      <Table>
        <TableHeader className="sticky top-0 bg-[#F7F3EA]"><TableRow>{(header || []).map((cell, i) => <TableHead key={i}>{cell}</TableHead>)}</TableRow></TableHeader>
        <TableBody>{matching.slice(0, csvRowCap).map((row, i) => <TableRow key={i}>{row.map((cell, j) => <TableCell key={j} className="font-mono text-[13px]">{cell}</TableCell>)}</TableRow>)}</TableBody>
      </Table>
    </div>
    {matching.length > csvRowCap && <div className="border-t bg-[#FBF9F4] px-3.5 py-3 text-[13px] text-muted-foreground">Showing the first {number(csvRowCap)} rows. Filter, or download the CSV for all of them.</div>}
  </div>
}
