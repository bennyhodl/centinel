import { useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Route } from './routes/_workspace/_admin/history'
import { api, type RunAttempt } from './api'
import { queries } from './queries'
import { useJobs, useOpenJob } from './jobs'
import { activeJobs } from './job-state'
import { number, seconds, when } from './format'
import { Empty, ErrorBox, PageHeader, SectionRule, Spinner } from './ui'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Badge } from '@/components/ui/badge'

export function CollectionHistory() {
  const search = Route.useSearch()
  const navigate = Route.useNavigate()
  const { data, error, isPlaceholderData } = useQuery(queries.collectionHistory(search))
  const open = useOpenJob()
  const live = activeJobs(useJobs()).find(job => job.kind === 'run')
  const [handle, setHandle] = useState('')
  return <>
    <PageHeader title="Collection history" detail="Every collection attempt, manual or scheduled. Classifier runs are listed separately under Runs." />
    <div className="grid max-w-6xl gap-8">
      {data?.running && <div className="flex flex-wrap items-center justify-between gap-3 rounded-md border p-4 text-sm">
        <span>Collecting {(data.running.sources ?? []).join(', ') || 'every enabled source'} since {when(data.running.started_at)}.</span>
        {live && <Button variant="outline" size="sm" onClick={() => open(live.id)}>Open live job</Button>}
      </div>}
      <form className="flex flex-wrap items-end gap-3" onSubmit={event => {
        event.preventDefault()
        const values = new FormData(event.currentTarget)
        navigate({ search: { ...search, source: String(values.get('source') || '').trim(), schedule: String(values.get('schedule') || '').trim(), outcome: String(values.get('outcome') || ''), page: 1 } })
      }}>
        <label className="grid gap-1 text-xs">Source<Input name="source" defaultValue={search.source} key={`source-${search.source}`} placeholder="All sources" /></label>
        <label className="grid gap-1 text-xs">Schedule<Input name="schedule" defaultValue={search.schedule} key={`schedule-${search.schedule}`} placeholder="All schedules" /></label>
        <label className="grid gap-1 text-xs">Outcome<select name="outcome" defaultValue={search.outcome} key={search.outcome} className="h-9 rounded-md border bg-background px-3 text-sm">
          <option value="">All outcomes</option><option value="ok">OK</option><option value="failed">Failed, partial or interrupted</option><option value="partial">Partial</option><option value="interrupted">Interrupted</option><option value="skipped">Skipped</option>
        </select></label>
        <Button variant="outline" type="submit">Apply filters</Button>
      </form>
      <form className="flex items-end gap-3" onSubmit={event => { event.preventDefault(); if (handle.trim()) navigate({ search: { ...search, run: handle.trim() } }) }}>
        <label className="grid flex-1 gap-1 text-xs sm:max-w-sm">Run id or prefix<Input value={handle} onChange={event => setHandle(event.target.value)} placeholder="2026-10-14T" /></label>
        <Button variant="outline" type="submit" disabled={!handle.trim()}>Open attempt</Button>
      </form>
      {search.run && <AttemptDetail id={search.run} onClose={() => navigate({ search: { ...search, run: '' } })} busy={Boolean(data?.running)} />}
      <section>
        <SectionRule aside={data ? `${number(data.total)} attempts` : undefined}>Collection attempts</SectionRule>
        {error && <ErrorBox error={error} />}
        {!data && !error && <Spinner />}
        <div className="overflow-x-auto">
          <table className="w-full text-left text-sm">
            <thead className="border-b text-xs text-muted-foreground"><tr>{['Started', 'Scope / trigger', 'Outcome', 'Added', 'Listing / request changes', 'Elapsed'].map(label => <th key={label} className="py-3 pr-5 font-normal">{label}</th>)}</tr></thead>
            <tbody>{data?.attempts.map(attempt => <tr key={attempt.run_id} className="border-b align-top hover:bg-hover">
              <td className="whitespace-nowrap py-4 pr-5"><button className="font-mono text-xs underline underline-offset-4" onClick={() => navigate({ search: { ...search, run: attempt.run_id } })}>{when(attempt.started_at)}</button></td>
              <td className="max-w-56 py-4 pr-5"><p>{scope(attempt)}</p><p className="mt-1 text-xs text-muted-foreground">{trigger(attempt)}</p></td>
              <td className="py-4 pr-5"><Badge variant="outline">{attempt.outcome}</Badge></td>
              <td className="py-4 pr-5"><Counts values={attempt.added} /></td>
              <td className="py-4 pr-5"><Counts values={attempt.subtracted} /></td>
              <td className="whitespace-nowrap py-4">{elapsed(attempt)}</td>
            </tr>)}</tbody>
          </table>
        </div>
        {data && !data.attempts.length && <Empty>No collection attempts match these filters.</Empty>}
        {data && <div className="mt-4 flex items-center gap-4 text-sm">
          <Button variant="outline" disabled={search.page <= 1 || isPlaceholderData} onClick={() => navigate({ search: { ...search, page: search.page - 1 } })}>Previous</Button>
          <span>Page {data.page} of {Math.max(1, Math.ceil(data.total / data.page_size))}</span>
          <Button variant="outline" disabled={data.page * data.page_size >= data.total || isPlaceholderData} onClick={() => navigate({ search: { ...search, page: search.page + 1 } })}>Next</Button>
        </div>}
      </section>
    </div>
  </>
}

function AttemptDetail({ id, onClose, busy }: { id: string; onClose: () => void; busy: boolean }) {
  const { data, error } = useQuery(queries.collectionAttempt(id))
  const open = useOpenJob()
  const client = useQueryClient()
  const rerun = useMutation({ mutationFn: () => api.rerunCollection(data!.attempt.run_id), onSuccess: ({ run_id }) => {
    open(run_id)
    client.invalidateQueries({ queryKey: ['collection-history'] })
    client.invalidateQueries({ queryKey: ['sources'] })
  } })
  const attempt = data?.attempt
  return <section className="min-w-0 rounded-md border p-5">
    <SectionRule aside={<Button variant="ghost" size="sm" onClick={onClose}>Close</Button>}>Attempt detail</SectionRule>
    {error && <ErrorBox error={error} />}
    {!attempt && !error && <Spinner />}
    {attempt && <div className="grid gap-4">
      <p className="break-all font-mono text-sm">{attempt.run_id}</p>
      <p className="text-sm">{scope(attempt)} · {trigger(attempt)} · {attempt.outcome} · {elapsed(attempt)}</p>
      <p className="text-xs text-muted-foreground">Started {attempt.started_at}<br />Finished {attempt.finished_at}{attempt.due_at && <><br />Due {attempt.due_at}</>}</p>
      {attempt.detail && <p className="whitespace-pre-wrap break-words text-sm">{attempt.detail}</p>}
      <div className="grid gap-4 sm:grid-cols-2"><div><h3 className="mb-2 text-sm font-semibold">Added</h3><Counts values={attempt.added} /></div><div><h3 className="mb-2 text-sm font-semibold">Listing / request changes</h3><Counts values={attempt.subtracted} /></div></div>
      <p className="text-xs text-muted-foreground">Vanished means no longer listed. Gone means HTTP 404 or 410. Blocked and errored describe requests. These counts do not mean stored documents were deleted.</p>
      {attempt.report != null && <details><summary className="cursor-pointer text-sm">Full collection report</summary><pre className="mt-3 max-h-96 overflow-auto rounded-md bg-muted p-3 text-xs">{JSON.stringify(attempt.report, null, 2)}</pre></details>}
      <div className="grid justify-items-start gap-2 border-t pt-4">
        <p className="text-sm">Run {scope(attempt)} with the current pipeline defaults. The journal retains source scope, not the original limits, skipped stages, refresh setting or configuration. An all-source attempt uses the sources enabled now.</p>
        <Button variant="outline" disabled={busy || rerun.isPending} onClick={() => rerun.mutate()}>{rerun.isPending ? 'Starting…' : 'Run this scope again'}</Button>
        {busy && <p className="text-xs text-muted-foreground">Wait for the current collection to finish before starting another.</p>}
        {rerun.error && <ErrorBox error={rerun.error} />}
      </div>
    </div>}
  </section>
}

const scope = (attempt: RunAttempt) => attempt.sources?.length ? attempt.sources.join(', ') : 'every enabled source'
const trigger = (attempt: RunAttempt) => attempt.trigger === 'manual' ? 'Manual' : `${attempt.trigger === 'catch_up' ? 'Catch-up' : 'Scheduled'} · ${attempt.schedule ?? 'unknown schedule'}`
const elapsed = (attempt: RunAttempt) => seconds(Date.parse(attempt.finished_at) - Date.parse(attempt.started_at))
function Counts({ values }: { values: Record<string, number> }) {
  return <div className="grid gap-1 text-xs tabular-nums">{Object.entries(values).map(([label, count]) => <span key={label}>{number(count)} {label}</span>)}</div>
}
