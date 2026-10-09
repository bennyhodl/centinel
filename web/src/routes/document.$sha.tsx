import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { createFileRoute } from '@tanstack/react-router'
import { ArrowLeft, RotateCcw } from 'lucide-react'
import { api } from '../api'
import { number, tail } from '../format'
import { ErrorBox, PageHeader } from '../ui'
import { Button } from '@/components/ui/button'

export const Route = createFileRoute('/document/$sha')({
  validateSearch: (search: Record<string, unknown>) => ({ source: String(search.source || ''), resource: String(search.resource || '') }),
  component: Reader,
})

function Reader() {
  const { sha } = Route.useParams()
  const search = Route.useSearch()
  const doc = { derived_sha: sha, source: search.source, resource: search.resource }
  const query = useQuery({ queryKey: ['read', doc], queryFn: () => api.read(doc) })
  const client = useQueryClient()
  const restore = useMutation({ mutationFn: () => api.restore(doc), onSuccess: () => client.invalidateQueries({ queryKey: ['corpus'] }) })
  return <>
    <PageHeader eyebrow={query.data?.source || search.source || 'Document'} title={query.data ? tail(query.data.url) : tail(search.resource || sha)}>
      <Button variant="secondary" onClick={() => history.back()}><ArrowLeft />Back</Button>
    </PageHeader>
    {query.error ? <ErrorBox error={query.error} /> : query.data ? <div className="grid items-start gap-4 xl:grid-cols-[minmax(0,1fr)_292px]">
      <article className="min-w-0 rounded-lg border bg-card [&_pre]:whitespace-pre-wrap [&_pre]:wrap-anywhere [&_pre]:p-6 [&_pre]:font-sans [&_pre]:text-sm [&_pre]:leading-7"><div className="flex min-h-11 flex-wrap items-center gap-4 border-b px-4 py-2 font-mono text-xs text-muted-foreground"><span>{query.data.kind}</span><span>{number(query.data.total_chars)} characters</span><span>{query.data.observed_at}</span></div><pre>{query.data.text}</pre></article>
      <aside className="rounded-lg border bg-card p-5 [&_h2]:text-lg [&_h2]:font-semibold [&_dl]:my-4 [&_dt]:mt-3 [&_dt]:text-xs [&_dt]:text-muted-foreground [&_dd]:mt-1 [&_dd]:text-sm [&_dd]:wrap-anywhere"><h2>Provenance</h2><dl><dt>Resource</dt><dd>{query.data.url}</dd><dt>Blob SHA</dt><dd className="font-mono">{query.data.blob_sha}</dd><dt>Text SHA</dt><dd className="font-mono">{query.data.derived_sha}</dd><dt>Extractor</dt><dd>{query.data.tool}</dd></dl><Button variant="secondary" className="w-full" disabled={restore.isPending} onClick={() => restore.mutate()}><RotateCcw />Restore usage</Button>{restore.isSuccess && <p className="mt-2 text-sm">Restored to corpus usage.</p>}{restore.error && <ErrorBox error={restore.error} />}</aside>
    </div> : <div className="flex items-center gap-2 py-6 text-sm text-muted-foreground">Reading the complete document…</div>}
  </>
}

