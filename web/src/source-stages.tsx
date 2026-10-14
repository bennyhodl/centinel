import { useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { api, type LaneHolder, type SourceStage } from './api'
import { useJobs, useOpenJob } from './jobs'
import { jobTitle } from './job-state'
import { ErrorBox, SectionRule, Spinner } from './ui'
import { Button } from '@/components/ui/button'

const stages: Array<{ stage: SourceStage; label: string; description: string }> = [
  { stage: 'extract', label: 'Extract', description: 'Derive text from collected documents.' },
  { stage: 'transcribe', label: 'Transcribe', description: 'Turn collected recordings into text.' },
  { stage: 'index', label: 'Index', description: 'Add derived text to keyword search.' },
  { stage: 'embed', label: 'Embed', description: 'Embed indexed chunks for semantic search.' },
]

/** Run one stage over this source's stored material, then follow it in the job drawer. */
export function SourceStages({ id, lane }: { id: string; lane: LaneHolder | null }) {
  const [refresh, setRefresh] = useState(false)
  const open = useOpenJob()
  const client = useQueryClient()
  const jobs = useJobs()
  const start = useMutation({
    mutationFn: (stage: SourceStage) => api.startSourceStage(id, stage, refresh && (stage === 'extract' || stage === 'transcribe')),
    onSuccess: ({ id: job }) => {
      open(job)
      client.invalidateQueries({ queryKey: ['source'] })
      client.invalidateQueries({ queryKey: ['sources'] })
    },
  })
  const job = jobs.find(job => !job.outcome && job.label === id && stages.some(({ stage }) => stage === job.kind))
  const busy = Boolean(lane || job) || start.isPending
  return <section className="grid gap-3">
    <SectionRule>Process stored material</SectionRule>
    <p className="text-sm text-muted-foreground">Run a stage on this source's collected material. Transcription and embedding use the configured inference servers.</p>
    <div className="grid sm:grid-cols-2">
      {stages.map(({ stage, label, description }) => <div key={stage} className="flex items-center gap-4 border-b py-3 sm:odd:pr-6 sm:even:pl-6">
        <span className="grid min-w-0 flex-1 gap-0.5"><b className="text-sm font-semibold">{label}</b><span className="text-xs text-muted-foreground">{description}</span></span>
        <Button size="sm" variant="outline" disabled={busy} onClick={() => start.mutate(stage)} aria-label={`Start ${label.toLowerCase()}`}>
          {start.isPending && start.variables === stage && <Spinner />}Start
        </Button>
      </div>)}
    </div>
    <label className="flex items-center gap-2 text-sm"><input type="checkbox" checked={refresh} disabled={busy} onChange={event => setRefresh(event.target.checked)} className="accent-foreground" />Reprocess existing text and transcripts</label>
    {job && <Button size="sm" variant="outline" className="justify-self-start" onClick={() => open(job.id)}>{jobTitle(job)} · Open progress</Button>}
    {lane && !job && <p className="text-xs text-muted-foreground">Another operation holds the collection lane. These stages are available when it finishes.</p>}
    {start.error && <ErrorBox error={start.error} />}
  </section>
}
