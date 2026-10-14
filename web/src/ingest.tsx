import { useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Link } from '@tanstack/react-router'
import { Download, Square } from 'lucide-react'
import { api } from './api'
import { queries } from './queries'
import { useJobs, useOpenJob } from './jobs'
import { ErrorBox, PageHeader, SectionRule, Spinner } from './ui'
import { Button } from '@/components/ui/button'
import { Field, FieldDescription, FieldLabel } from '@/components/ui/field'
import { Textarea } from '@/components/ui/textarea'

/** Fetch explicit addresses into an existing source, without crawling or indexing. */
export function Ingest() {
  const { data, error } = useQuery(queries.sources())
  const client = useQueryClient()
  const open = useOpenJob()
  const [source, setSource] = useState('')
  const [text, setText] = useState('')
  const [id, setId] = useState('')
  const job = useJobs().find(job => job.id === id)
  const urls = text.split(/\r?\n/).map(line => line.trim()).filter(Boolean)
  const invalid = urls.find(address => {
    try {
      const url = new URL(address)
      return !['http:', 'https:'].includes(url.protocol) || !url.hostname || Boolean(url.username || url.password)
    } catch { return true }
  })
  const ingest = useMutation({
    mutationFn: () => api.ingest({ source, urls }),
    onSuccess: ({ id }) => {
      setId(id)
      stop.reset()
      open(id)
      client.invalidateQueries({ queryKey: ['sources'] })
    },
    onError: () => client.invalidateQueries({ queryKey: ['sources'] }),
  })
  const stop = useMutation({ mutationFn: () => api.stopJob(id) })
  const stopping = stop.isPending || stop.isSuccess
  const busy = ingest.isPending || Boolean(id && !job?.outcome)
  return <>
    <PageHeader title="Ingest URLs" detail="Fetch specific pages or files and archive their bytes under an existing source." />
    <div className="grid max-w-2xl gap-8">
      <section className="grid gap-5">
        <SectionRule>Acquisition</SectionRule>
        <p className="text-sm text-muted-foreground">This stores the URLs you enter. Run extraction and indexing afterwards to make their content searchable.</p>
        {error && <ErrorBox error={error} />}
        {data && !data.sources.length && <p className="text-sm">Add a source on the <Link to="/sources" className="underline">Sources page</Link> before ingesting URLs.</p>}
        <form className="grid gap-5" onSubmit={event => { event.preventDefault(); if (!invalid && urls.length) ingest.mutate() }}>
          <Field>
            <FieldLabel htmlFor="ingest-source">Source</FieldLabel>
            <select id="ingest-source" required value={source} onChange={event => setSource(event.target.value)} className="h-9 w-full rounded-md border border-input bg-background px-3 text-sm focus-visible:outline-2 focus-visible:outline-ring">
              <option value="">Choose an existing source</option>
              {(data?.sources ?? []).map(source => <option key={source.id} value={source.id}>{source.id}</option>)}
            </select>
            <FieldDescription>Each fetched URL belongs to this source. Ingestion does not change its collection settings.</FieldDescription>
          </Field>
          <Field>
            <FieldLabel htmlFor="ingest-urls">URLs</FieldLabel>
            <Textarea id="ingest-urls" required rows={8} value={text} onChange={event => setText(event.target.value)} placeholder={'https://www.tampa.gov/agenda.pdf\nhttps://www.tampa.gov/meeting-minutes'} aria-invalid={Boolean(invalid)} aria-describedby="ingest-url-help" className="font-mono text-[13px]" />
            <FieldDescription id="ingest-url-help">One HTTP or HTTPS URL per line. Local files and URLs with credentials are not accepted.</FieldDescription>
            {invalid && <p role="alert" className="break-all text-sm text-destructive">Enter an HTTP or HTTPS URL without credentials: {invalid}</p>}
          </Field>
          {data?.running && <p role="status" className="text-sm text-flame-ink">A collection or acquisition job holds the lane. Wait for it to finish before ingesting URLs.</p>}
          {stop.error && <ErrorBox error={stop.error} />}
          {ingest.error && <ErrorBox error={ingest.error} />}
          <div className="flex flex-wrap items-center gap-3">
            <Button type="submit" disabled={!source || !urls.length || Boolean(invalid || data?.running) || busy}>{busy ? <Spinner /> : <Download />}Ingest {urls.length ? `${urls.length} URL${urls.length === 1 ? '' : 's'}` : 'URLs'}</Button>
            {job?.stoppable && !job.outcome && <Button type="button" variant="outline" disabled={stopping} onClick={() => stop.mutate()}>{stopping ? <Spinner /> : <Square />}{stopping ? 'Stopping after the current URL' : 'Stop'}</Button>}
            {id && <Button type="button" variant="outline" onClick={() => open(id)}>View progress</Button>}
          </div>
          {job?.outcome && <p role="status" className="text-sm text-muted-foreground">{job.outcome === 'ok' ? `${job.ok} URLs stored. Extraction and indexing are still needed.` : job.outcome === 'cancelled' ? 'Ingestion stopped. URLs already stored were retained.' : job.error || 'Ingestion failed. Open progress for details.'}</p>}
        </form>
      </section>
    </div>
  </>
}
