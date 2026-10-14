import { useState } from 'react'
import { useMutation, useQueryClient, useSuspenseQuery } from '@tanstack/react-query'
import { Pause, Play, Plus, Trash2 } from 'lucide-react'
import { api, type ScheduleDefinition, type ScheduleRow } from './api'
import { queries } from './queries'
import { useOpenJob } from './jobs'
import { ErrorBox, PageHeader, SectionRule, Spinner } from './ui'
import { Button } from './components/ui/button'
import { Input } from './components/ui/input'
import { Dialog, DialogContent, DialogHeader, DialogTitle } from './components/ui/dialog'

const empty: ScheduleDefinition = { id: '', cron: '0 3 * * *', tz: 'America/New_York', sources: [], enabled: true }
const fireTime = (time: string, zone: string) => new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short', timeZone: zone === 'local' ? undefined : zone }).format(new Date(time))

export function Schedules() {
  const { data } = useSuspenseQuery(queries.schedules())
  const [editing, setEditing] = useState<ScheduleDefinition | null>(null)
  const [creating, setCreating] = useState(false)
  return <>
    <PageHeader title="Schedules" detail="Choose when collection runs and which sources it covers.">
      <Button onClick={() => { setCreating(true); setEditing(empty) }}><Plus />Add a schedule</Button>
    </PageHeader>
    <div className="grid max-w-5xl gap-7">
      {!data.scheduler.enabled && <p role="status" className="rounded-md border bg-muted p-4 text-sm">Automatic collection is off ({data.scheduler.disabled_reason}). Changes are saved for the next scheduled server. Run now starts a manual collection.</p>}
      <section>
        <SectionRule aside={`${data.schedules.length} schedules`}>Collection times</SectionRule>
        {data.schedules.length === 0 && <p className="py-8 text-sm text-muted-foreground">No schedules yet. Add one to collect sources on a regular cadence.</p>}
        {data.schedules.map(row => <Schedule key={row.definition.id} row={row} laneHeld={Boolean(data.running)} automatic={data.scheduler.enabled} onEdit={() => { setCreating(false); setEditing(row.definition) }} />)}
      </section>
    </div>
    {editing && <ScheduleEditor definition={editing} creating={creating} automatic={data.scheduler.enabled} onClose={() => setEditing(null)} />}
  </>
}

function Schedule({ row, laneHeld, automatic, onEdit }: { row: ScheduleRow; laneHeld: boolean; automatic: boolean; onEdit: () => void }) {
  const { definition: schedule, status, next } = row
  const client = useQueryClient()
  const open = useOpenJob()
  const refresh = () => { client.invalidateQueries({ queryKey: ['schedules'] }); client.invalidateQueries({ queryKey: ['sources'] }) }
  const enable = useMutation({ mutationFn: () => api.setScheduleEnabled(schedule.id, !status.enabled), onSuccess: refresh })
  const remove = useMutation({ mutationFn: () => api.removeSchedule(schedule.id), onSuccess: refresh })
  const run = useMutation({ mutationFn: () => api.runSchedule(schedule.id), onSuccess: ({ run_id }) => { open(run_id); refresh() } })
  const [removing, setRemoving] = useState(false)
  const error = enable.error ?? remove.error ?? run.error
  const busy = enable.isPending || remove.isPending || run.isPending
  return <article className="grid gap-4 border-b py-5 md:grid-cols-[1fr_1fr]">
    <div className="grid content-start gap-2">
      <div className="flex items-center gap-3"><h2 className="font-semibold">{schedule.id}</h2><span className="text-xs text-muted-foreground">{status.running ? 'Collecting' : !status.enabled ? 'Paused' : automatic ? 'Active' : 'Saved'}</span></div>
      <p className="text-sm"><code className="font-mono">{schedule.cron}</code><span className="ml-3 text-muted-foreground">{status.tz}</span></p>
      <p className="text-sm text-muted-foreground">{schedule.sources?.length ? schedule.sources.join(', ') : 'Every enabled source'}</p>
      <p className="text-xs text-muted-foreground">Last outcome: {status.last_outcome ?? 'Never run'}{status.last_fire && ` · ${fireTime(status.last_fire, status.tz)}`}</p>
      <div className="flex flex-wrap gap-2 pt-1">
        <Button size="sm" variant="outline" onClick={onEdit} disabled={busy}>Edit</Button>
        <Button size="sm" variant="outline" onClick={() => enable.mutate()} disabled={busy}>{status.enabled ? <Pause /> : <Play />}{status.enabled ? 'Pause' : 'Resume'}</Button>
        <Button size="sm" variant="outline" disabled={laneHeld || busy} title={laneHeld ? 'Another collection holds the lane' : undefined} onClick={() => run.mutate()}>{run.isPending ? <Spinner /> : <Play />}Run now</Button>
        <Button size="sm" variant="ghost" disabled={busy} onClick={() => setRemoving(true)}><Trash2 />Remove</Button>
      </div>
      {removing && <div className="flex flex-wrap items-center gap-2 text-sm"><span>Remove this schedule? Collection history stays.</span><Button size="sm" variant="destructive" disabled={busy} onClick={() => remove.mutate()}>Remove schedule</Button><Button size="sm" variant="ghost" onClick={() => setRemoving(false)}>Cancel</Button></div>}
      {error && <ErrorBox error={error} />}
    </div>
    <div className="grid content-start gap-2 border-l pl-5">
      <p className="text-xs text-muted-foreground">{status.enabled && automatic ? 'Next three collections' : 'Cadence preview, automatic collection is off'}</p>
      <ol className="grid gap-1.5 text-sm tabular-nums">{next.map(time => <li key={time}><time dateTime={time}>{fireTime(time, status.tz)}</time></li>)}</ol>
      {status.problem && <p className="text-sm text-destructive">{status.problem}</p>}
    </div>
  </article>
}

function ScheduleEditor({ definition, creating, automatic, onClose }: { definition: ScheduleDefinition; creating: boolean; automatic: boolean; onClose: () => void }) {
  const [draft, setDraft] = useState(definition)
  const { data } = useSuspenseQuery(queries.sources())
  const client = useQueryClient()
  const save = useMutation({ mutationFn: () => api.saveSchedule(draft, creating), onSuccess: () => { client.invalidateQueries({ queryKey: ['schedules'] }); onClose() } })
  const chosen = draft.sources ?? []
  return <Dialog open onOpenChange={open => { if (!open && !save.isPending) onClose() }}>
    <DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-lg">
      <DialogHeader><DialogTitle>{creating ? 'Add a schedule' : `Edit ${definition.id}`}</DialogTitle></DialogHeader>
      <form className="grid gap-5" onSubmit={event => { event.preventDefault(); save.mutate() }}>
        <label className="grid gap-2 text-sm">Schedule id<Input required value={draft.id} disabled={!creating} onChange={event => setDraft({ ...draft, id: event.target.value })} placeholder="tampa-daily" /></label>
        <label className="grid gap-2 text-sm">Cron expression<Input required className="font-mono" value={draft.cron} onChange={event => setDraft({ ...draft, cron: event.target.value })} /><span className="text-xs text-muted-foreground">Minute, hour, day of month, month, weekday. For example, 0 3 * * * runs daily at 3 am.</span></label>
        <label className="grid gap-2 text-sm">Timezone<Input value={draft.tz ?? ''} onChange={event => setDraft({ ...draft, tz: event.target.value || null })} placeholder="Host timezone" /></label>
        <fieldset className="grid gap-2"><legend className="mb-2 text-sm">Sources</legend><p className="mb-2 text-xs text-muted-foreground">Leave all unchecked to collect every enabled source.</p>
          <div className="grid max-h-44 gap-2 overflow-y-auto">{data.sources.map(source => <label key={source.id} className="flex items-center gap-2 text-sm"><input type="checkbox" checked={chosen.includes(source.id)} onChange={event => setDraft({ ...draft, sources: event.target.checked ? [...chosen, source.id] : chosen.filter(id => id !== source.id) })} />{source.id}{!source.enabled && <span className="text-xs text-muted-foreground">disabled</span>}</label>)}</div>
          {chosen.filter(id => !data.sources.some(source => source.id === id)).map(id => <label key={id} className="flex items-center gap-2 text-sm text-destructive"><input type="checkbox" checked onChange={() => setDraft({ ...draft, sources: chosen.filter(source => source !== id) })} />{id} (missing source)</label>)}
        </fieldset>
        <label className="flex items-center gap-2 text-sm"><input type="checkbox" checked={draft.enabled !== false} onChange={event => setDraft({ ...draft, enabled: event.target.checked })} />Enable this schedule</label>
        <p className="text-xs text-muted-foreground">{automatic ? 'Changes apply to this server before its next collection.' : 'Automatic collection is off on this server. Changes are saved for the next scheduled server.'} Fire times include this installation's configured jitter.</p>
        {save.error && <ErrorBox error={save.error} />}
        <div className="flex justify-end gap-2"><Button type="button" variant="outline" disabled={save.isPending} onClick={onClose}>Cancel</Button><Button disabled={save.isPending}>{save.isPending && <Spinner />}Save schedule</Button></div>
      </form>
    </DialogContent>
  </Dialog>
}
