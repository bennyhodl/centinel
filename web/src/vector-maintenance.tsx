import { useState } from 'react'
import { useMutation, useQuery } from '@tanstack/react-query'
import { Check, Play, ScanSearch, Wrench } from 'lucide-react'
import { api, type VectorReport } from './api'
import { Bar, useJobs, useOpenJob } from './jobs'
import { jobTitle } from './job-state'
import { number, plural } from './format'
import { queries } from './queries'
import { Empty, ErrorBox, PageHeader, SectionRule, Spinner } from './ui'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'

export function VectorMaintenance({ reportId, onReport }: { reportId: string; onReport: (id: string) => void }) {
  const openJob = useOpenJob()
  const jobs = useJobs()
  const job = jobs.find(candidate => candidate.id === reportId)
  const active = jobs.find(candidate => candidate.kind.startsWith('vectors-') && !candidate.outcome)
  const result = useQuery(queries.vectorReport(reportId))
  const [confirmRepair, setConfirmRepair] = useState(false)
  const [storageChecked, setStorageChecked] = useState(false)
  const [embedId, setEmbedId] = useState('')
  const embedJob = jobs.find(candidate => candidate.id === embedId)
  const started = ({ id }: { id: string }) => { onReport(id); openJob(id); setStorageChecked(false); setEmbedId('') }
  const verify = useMutation({ mutationFn: api.verifyVectors, onSuccess: value => { repair.reset(); embed.reset(); started(value) } })
  const repair = useMutation({ mutationFn: () => api.repairVectors(reportId), onSuccess: value => { setConfirmRepair(false); started(value) } })
  const embed = useMutation({ mutationFn: () => api.embedVectors(reportId), onSuccess: ({ id }) => { setEmbedId(id); openJob(id) } })
  const busy = Boolean(active) || verify.isPending || repair.isPending || embed.isPending
  const report = result.data?.report
  const unreadable = report?.unreadable ?? []
  const error = verify.error || repair.error || embed.error || (job?.error ? new Error(job.error) : result.error)

  return <>
    <PageHeader title="Vector maintenance" detail="Read every vector fragment back before choosing a repair. The audit leaves stored vectors unchanged.">
      <Button disabled={busy} onClick={() => verify.mutate()}>{verify.isPending ? <Spinner /> : <ScanSearch />}Verify vectors</Button>
    </PageHeader>
    {error && <ErrorBox error={error} />}
    {active && <div className="mb-7 grid gap-3 rounded-lg border p-4" aria-live="polite">
      <div className="flex flex-wrap items-center justify-between gap-3"><span className="flex items-center gap-2 text-sm"><Spinner />{active.message || active.step || jobTitle(active)}</span><Button variant="outline" size="sm" onClick={() => openJob(active.id)}>Open job</Button></div>
      <Bar job={active} />
    </div>}
    <SectionRule aside={report?.version ? `Audited table version ${report.version}` : undefined}>Verification report</SectionRule>
    {!report && !error && <Empty>{reportId ? 'The report will appear when this job finishes.' : 'Run verification to check the derived vector table. No archive files are removed by maintenance.'}</Empty>}
    {report && <div className="grid gap-7 pt-5">
      <div className="grid gap-2">
        <h2 className={`flex items-center gap-2 text-xl font-semibold ${unreadable.length && !report.repaired ? 'text-destructive' : ''}`}>
          {report.repaired ? <><Check className="size-5 text-moss" />Repair completed</> : unreadable.length ? `${plural(unreadable.length, 'unreadable fragment')}` : report.fragments ? <><Check className="size-5 text-moss" />Every fragment reads</> : 'No vector table yet'}
        </h2>
        <p className="text-sm text-muted-foreground">{report.repaired && 'Audit before repair: '}{number(report.fragments)} fragments checked · {number(report.readable)} readable · {number(unreadable.length)} unreadable</p>
        <p className="font-mono text-xs text-muted-foreground wrap-anywhere">{report.vectors}</p>
      </div>
      {unreadable.length > 0 && <Fragments report={report} />}
      {unreadable.length > 0 && !report.repaired && <div className="grid justify-items-start gap-3 border-t pt-5">
        <p className="max-w-2xl text-sm">Repair removes the unreadable fragments from the derived vector table. Their chunks can be embedded again from the index. Archived documents and source logs stay intact.</p>
        <p className="text-sm text-muted-foreground">Repair rechecks this report against the current table. If the table or findings changed, run verification again.</p>
        <Button variant="destructive" disabled={busy} onClick={() => setConfirmRepair(true)}><Wrench />Review repair</Button>
      </div>}
      {report.repaired && <div className="grid justify-items-start gap-4 border-t pt-5">
        <p className="text-sm">Removed {plural(report.repaired.fragments_dropped, 'derived vector fragment')}, approximately {plural(report.repaired.estimated_rows, 'vector row')}. No archive files were deleted.</p>
        {report.repaired.fragments_dropped > 0 && <>
          <p className="max-w-2xl text-sm text-muted-foreground">Check the storage that lost these writes before embedding again. The normal embed stage uses the configured model for missing indexed chunks, removes vectors no longer in the index, and maintains the table. Model calls may incur costs.</p>
          <label className="flex items-center gap-2 text-sm"><input type="checkbox" checked={storageChecked} onChange={event => setStorageChecked(event.target.checked)} className="size-4 accent-foreground" />I checked the storage and am ready to embed missing chunks.</label>
          <Button disabled={busy || !storageChecked || Boolean(embedId && !embedJob?.outcome)} onClick={() => embed.mutate()}><Play />Re-embed missing chunks</Button>
          {embedId && <div className="flex flex-wrap items-center gap-3 text-sm"><span aria-live="polite">{embedJob?.outcome === 'ok' ? 'Embedding finished.' : embedJob?.outcome ? `Embedding ${embedJob.outcome}.` : 'Embedding started.'}</span><Button size="sm" variant="outline" onClick={() => openJob(embedId)}>Open job</Button></div>}
        </>}
      </div>}
      <p className="text-xs text-muted-foreground">Report {reportId}{job?.finished_at ? ` · ${new Date(job.finished_at).toLocaleString()}` : ''}. Run verification again for a current audit.</p>
    </div>}
    {confirmRepair && <Dialog open onOpenChange={setConfirmRepair}>
      <DialogContent>
        <DialogHeader><DialogTitle>Remove unreadable derived vectors?</DialogTitle><DialogDescription>This removes {plural(unreadable.length, 'fragment')} from the vector table. Archived documents remain available. Re-embedding is a separate action after you check the storage.</DialogDescription></DialogHeader>
        {repair.error && <ErrorBox error={repair.error} />}
        <DialogFooter><Button variant="outline" onClick={() => setConfirmRepair(false)}>Cancel</Button><Button variant="destructive" disabled={busy} onClick={() => repair.mutate()}>{repair.isPending ? <Spinner /> : <Wrench />}Remove unreadable vectors</Button></DialogFooter>
      </DialogContent>
    </Dialog>}
  </>
}

function Fragments({ report }: { report: VectorReport }) {
  return <div className="grid gap-3">
    <SectionRule>{report.repaired ? 'Removed fragments' : 'Unreadable fragments'}</SectionRule>
    <ul className="divide-y">{report.unreadable?.map(fragment => <li key={fragment.id} className="grid gap-2 py-4">
      <div className="flex flex-wrap items-baseline gap-3"><span className="font-mono text-sm font-semibold">Fragment {fragment.id}</span><span className="text-xs text-muted-foreground">{fragment.physical_rows === undefined ? 'Unknown row count' : `Approximately ${plural(fragment.physical_rows, 'row')}`}</span></div>
      <ul className="grid gap-1">{fragment.data_files.map(file => <li key={file} className="font-mono text-xs wrap-anywhere">{file}</li>)}</ul>
      <p className="whitespace-pre-wrap text-xs text-destructive wrap-anywhere">{fragment.error}</p>
    </li>)}</ul>
  </div>
}
