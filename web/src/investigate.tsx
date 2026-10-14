import { useState, type ReactNode } from 'react'
import { useMutation, useQuery } from '@tanstack/react-query'
import { Link, useNavigate } from '@tanstack/react-router'
import { Check, ChevronLeft, ClipboardCopy, Plus, Search, X } from 'lucide-react'
import { cn } from 'cn'
import { api, type InvestigationReport, type NewSource, type RequestRecord } from './api'
import { AddSource } from './add-source'
import { HostIcon } from './host-icon'
import { useJobs } from './jobs'
import { bytes, clock, plural } from './format'
import { agentReport, frontPage, glanceOf, hostOf, pathOf, summaryOf, timelineOf, typeOf, verdictOf, type Decisions, type Entry, type Verdict } from './investigate-logic'
import { queries } from './queries'
import { ErrorBox, SectionRule, Spinner } from './ui'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Field, FieldDescription, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Skeleton } from '@/components/ui/skeleton'

// ── starting one ─────────────────────────────────────────────────────────────

/**
 * Starts an investigation of an address and opens its page. The one way anything in the
 * workspace investigates: the Sources list, a crumb and a graph node all call this with a
 * URL, so they all land in the same place.
 */
export function useInvestigate() {
  const navigate = useNavigate()
  return useMutation({
    mutationFn: (address: string) => api.investigate(address.trim()),
    onSuccess: ({ id }) => navigate({ to: '/sources/investigate/$id', params: { id } }),
  })
}

/** Asks for an address, then investigates it. Nothing is added or stored in the corpus. */
export function InvestigateDialog({ initial = '', onClose }: { initial?: string; onClose: () => void }) {
  const [address, setAddress] = useState(initial)
  const investigate = useInvestigate()
  return <Dialog open onOpenChange={next => !next && onClose()}>
    <DialogContent>
      <form onSubmit={event => { event.preventDefault(); investigate.mutate(address, { onSuccess: onClose }) }} className="grid gap-5">
        <DialogHeader>
          <DialogTitle>Investigate an address</DialogTitle>
          <DialogDescription>Asks which collection method recognises the site, fetches the address and shows what extraction makes of it. Nothing is added or stored in the corpus.</DialogDescription>
        </DialogHeader>
        <Field>
          <FieldLabel htmlFor="investigate-address">Address</FieldLabel>
          <Input id="investigate-address" autoFocus value={address} onChange={event => setAddress(event.target.value)} placeholder="https://www.example.gov/" inputMode="url" required className="font-mono text-[13px]" />
          <FieldDescription>Keep the path: a method that walks a directory stays under the one it was pointed at.</FieldDescription>
        </Field>
        {investigate.error && <ErrorBox error={investigate.error} />}
        <DialogFooter>
          <Button type="button" variant="outline" onClick={onClose}>Cancel</Button>
          <Button type="submit" disabled={!address.trim() || investigate.isPending}>{investigate.isPending ? <Spinner /> : <Search />}Investigate</Button>
        </DialogFooter>
      </form>
    </DialogContent>
  </Dialog>
}

// ── the page ─────────────────────────────────────────────────────────────────

export function InvestigatePage({ id }: { id: string }) {
  const job = useJobs().find(candidate => candidate.id === id)
  const finished = !job || Boolean(job.outcome)
  const report = useQuery({ ...queries.investigation(id), enabled: finished })
  const [adding, setAdding] = useState<NewSource | null>(null)

  if (report.data) return <>
    <Result report={report.data} id={id} onAdd={setAdding} />
    {adding && <AddSource initial={adding} onClose={() => setAdding(null)} />}
  </>
  // Until the report is filed the page is its own skeleton, so the layout is the one the
  // report lands in. A failed job or an unreadable report replaces the spinner, not the page.
  const failed = job?.outcome === 'failed' && job.error ? new Error(job.error) : report.error
  return <InvestigateSkeleton address={job?.label} status={failed ? <ErrorBox error={failed} /> : <Working message={finished ? 'Reading the report' : job?.message || job?.step || 'Investigating'} />} />
}

function Working({ message }: { message: string }) {
  return <div className="mb-6 flex items-center gap-3 rounded-lg border px-4 py-3 text-sm"><Spinner /><span className="flex-1">{message}…</span></div>
}

function BackToSources() {
  return <Link to="/sources" className="mb-4 inline-flex items-center gap-1 text-[13px] text-muted-foreground hover:text-foreground"><ChevronLeft className="size-3.5" />All sources</Link>
}

/** The title and the line under it. The host is the title, in the face every page title wears. */
function Header({ address, subtitle, children }: { address: string; subtitle: ReactNode; children?: ReactNode }) {
  return <header className="mb-7 flex flex-col justify-between gap-6 sm:flex-row sm:items-end">
    <div className="min-w-0">
      <BackToSources />
      <div className="flex items-center gap-3">
        <HostIcon host={hostOf(address)} size={28} />
        <h1 className="font-serif text-[40px] leading-[44px] tracking-[-0.01em] wrap-anywhere">{hostOf(address)}</h1>
      </div>
      <p className="mt-2 text-sm text-muted-foreground wrap-anywhere">{subtitle}</p>
    </div>
    {children}
  </header>
}

function Result({ report, id, onAdd }: { report: InvestigationReport; id: string; onAdd: (source: NewSource) => void }) {
  const promote = report.investigate.report?.promote
  const verdict = verdictOf(report)
  const { requests, seconds } = summaryOf(report)
  const [copied, setCopied] = useState(false)
  const copy = () => navigator.clipboard.writeText(agentReport(report, id)).then(() => { setCopied(true); setTimeout(() => setCopied(false), 1500) })
  return <>
    <Header address={report.address} subtitle={`Investigated ${report.address} at ${clock(report.started_at)} · ${plural(requests, 'request')} in ${seconds.toFixed(1)} s · nothing was stored`}>
      <div className="flex shrink-0 items-center gap-2">
        {promote
          ? <Button variant="outline" onClick={() => onAdd({ id: promote.id, site: promote.site, strategy: promote.strategy })}><Plus />Add as {promote.id}</Button>
          : <Button variant="outline" onClick={() => onAdd({ id: '', site: report.address })}><Plus />Add anyway</Button>}
        <Button onClick={copy}><ClipboardCopy />{copied ? 'Copied' : 'Copy report for an agent'}</Button>
      </div>
    </Header>
    <div className="grid items-start gap-10 lg:grid-cols-[minmax(0,1fr)_320px]">
      <div className="grid min-w-0 grid-cols-1 gap-6">
        <VerdictBlock verdict={verdict} />
        <Timeline entries={timelineOf(report)} tone={verdict.tone} />
      </div>
      <aside className="grid gap-6">
        <Glance report={report} />
        <FrontPage report={report} />
      </aside>
    </div>
  </>
}

const toneInk = { ok: 'text-moss', warn: 'text-flame-ink', bad: 'text-destructive' }

function VerdictBlock({ verdict }: { verdict: Verdict }) {
  const Mark = verdict.tone === 'ok' ? Check : X
  return <div className="grid gap-2">
    <div className={cn('flex items-center gap-2 text-xs font-semibold uppercase tracking-[0.08em]', toneInk[verdict.tone])}><Mark className="size-3.5" />{verdict.label}</div>
    <h2 className="text-[26px] leading-8 font-semibold tracking-[-0.015em]">{verdict.headline}.</h2>
    {verdict.findings.map((finding, i) => <p key={i} className="text-sm text-ink-soft">{finding}</p>)}
  </div>
}

// ── what happened, in order ──────────────────────────────────────────────────

const restsOnStyle = {
  ok: 'bg-moss-soft shadow-[inset_3px_0_0_var(--moss)]',
  warn: 'bg-flame-soft shadow-[inset_3px_0_0_var(--flame)]',
  bad: 'bg-destructive-soft shadow-[inset_3px_0_0_var(--destructive)]',
}

function Timeline({ entries, tone }: { entries: Entry[]; tone: Verdict['tone'] }) {
  return <section className="rounded-[10px] bg-canvas py-2 shadow-[inset_0_0_0_1px_var(--rule)]">
    <div className="border-b px-5 pt-2.5 pb-3 text-[11px] font-semibold uppercase tracking-[0.08em] text-muted-foreground">What happened, in order</div>
    {entries.map((entry, i) => entry.type === 'request'
      ? <RequestRow key={i} entry={entry} tone={tone} />
      : <DecisionRows key={i} decisions={entry.decisions} />)}
    {!entries.length && <p className="px-5 py-4 text-sm text-muted-foreground">No request was recorded.</p>}
  </section>
}

function RequestRow({ entry, tone }: { entry: Extract<Entry, { type: 'request' }>; tone: Verdict['tone'] }) {
  const { record } = entry
  const meant = [typeOf(record.content_type), record.bytes || record.content_type ? bytes(record.bytes) : undefined, `${record.elapsed_ms} ms`].filter(Boolean).join(' · ')
  return <div className={cn('flex gap-4 px-5 py-3', entry.restsOn && restsOnStyle[tone])}>
    <span className="w-12 shrink-0 font-mono text-xs leading-[22px] text-muted-foreground">+{entry.offset.toFixed(2)}</span>
    <div className="flex min-w-0 flex-1 flex-col gap-1">
      <div className="flex items-center gap-2.5">
        <StatusChip record={record} />
        <a href={record.url} target="_blank" rel="noreferrer" className="min-w-0 flex-1 truncate font-mono text-[13px] hover:underline" title={record.url}>{record.method} {pathOf(record.url)}</a>
        <span className="shrink-0 font-mono text-xs text-muted-foreground">{meant}</span>
      </div>
      {entry.note && <p className={cn('text-[13px] leading-[19px]', entry.restsOn ? 'font-medium' : 'text-ink-soft', entry.restsOn && toneInk[tone])}>{entry.note}</p>}
    </div>
  </div>
}

function StatusChip({ record }: { record: RequestRecord }) {
  const bad = Boolean(record.error)
  return <span className={cn('inline-flex h-[22px] items-center rounded-[5px] px-[7px] font-mono text-xs font-semibold', bad ? 'bg-destructive-soft text-destructive' : 'bg-track')}>{record.status ?? '—'}</span>
}

/** Each method's decision, at the point the recognisers ran: a ruled box per method, so a refusal reads as plainly as a yes. */
function DecisionRows({ decisions }: { decisions: Decisions }) {
  return <div className="grid gap-2 px-5 pt-0.5 pb-2.5 pl-[84px]">
    {decisions.recognised.map(hit => <div key={hit.strategy} className="grid gap-1.5 rounded-md bg-background px-3 py-2 shadow-[inset_0_0_0_1px_var(--rule)]">
      <div className="flex items-center gap-2.5">
        <Check className="size-3 shrink-0 text-moss" strokeWidth={2.6} />
        <span className="w-[120px] shrink-0 font-mono text-xs">{hit.strategy}</span>
        <span className="min-w-0 flex-1 text-[13px] text-muted-foreground">{hit.keyedOn} · {hit.kind}{hit.best ? ' · would run' : ''}</span>
      </div>
      {hit.evidence.length > 0 && <ul className="grid gap-0.5 pl-[22px] text-[13px]">
        {hit.evidence.map((note, i) => <li key={i} className={note.mark === 'warn' ? 'text-flame-ink' : note.mark === 'bad' ? 'text-destructive' : 'text-ink-soft'}><span className="font-mono text-xs text-muted-foreground">{note.label}</span> {note.detail}</li>)}
      </ul>}
    </div>)}
    {decisions.declined.map(name => <div key={name} className="flex items-center gap-2.5 rounded-md bg-background px-3 py-2 shadow-[inset_0_0_0_1px_var(--rule)]">
      <X className="size-3 shrink-0 text-destructive" strokeWidth={2.6} />
      <span className="w-[120px] shrink-0 font-mono text-xs">{name}</span>
      <span className="min-w-0 flex-1 text-[13px] text-muted-foreground">did not recognise it</span>
    </div>)}
  </div>
}

// ── at a glance ──────────────────────────────────────────────────────────────

function Glance({ report }: { report: InvestigationReport }) {
  const rows = glanceOf(report)
  return <section>
    <SectionRule>At a glance</SectionRule>
    {report.investigate.error && <ErrorBox error={new Error(report.investigate.error)} />}
    {rows.map(row => <div key={row.label} className="flex justify-between gap-3 border-b py-2.5 text-[13px]">
      <span className="text-muted-foreground">{row.label}</span>
      <span className={cn('text-right', row.tone === 'bad' && 'font-semibold text-destructive')}>{row.value}</span>
    </div>)}
  </section>
}

function FrontPage({ report }: { report: InvestigationReport }) {
  const doc = frontPage(report)
  const findings = [...(doc?.verdict?.findings ?? []), ...(doc?.notes ?? [])]
  return <section className="grid gap-2.5">
    <SectionRule aside={doc && <a href={doc.address} target="_blank" rel="noreferrer" className="font-semibold text-foreground hover:underline">Open</a>}>The front page, as extracted</SectionRule>
    {report.check.error && <ErrorBox error={new Error(report.check.error)} />}
    {doc?.preview && <pre className="max-h-72 overflow-auto rounded-lg bg-muted px-3.5 py-3 font-mono text-[11px] leading-[17px] whitespace-pre-wrap text-ink-soft">{doc.preview}</pre>}
    {doc && !doc.preview && <p className="text-[13px] text-muted-foreground">{doc.unextractable ?? 'No text came out of it.'}</p>}
    {doc?.tool && <p className="text-xs leading-[17px] text-muted-foreground">Read by {doc.tool}.{findings.length ? ` ${findings.join(' ')}` : ''}</p>}
  </section>
}

// ── while it runs ────────────────────────────────────────────────────────────

/** The page's own shape with its data not yet here, so nothing moves when the report lands. */
export function InvestigateSkeleton({ address = '', status }: { address?: string; status?: ReactNode }) {
  return <>
    <Header address={address || 'https://example.gov/'} subtitle={<Skeleton mask="Investigated https://example.gov/ at 22:41 · 6 requests in 1.6 s · nothing was stored" />}>
      <div className="flex shrink-0 items-center gap-2"><Button variant="outline" disabled><Plus />Add anyway</Button><Button disabled><ClipboardCopy />Copy report for an agent</Button></div>
    </Header>
    {status}
    <div aria-busy className="grid items-start gap-10 lg:grid-cols-[minmax(0,1fr)_320px]">
      <div className="grid gap-6">
        <div className="grid gap-2">
          <span className="text-xs font-semibold uppercase tracking-[0.08em]"><Skeleton mask="Not recognised" /></span>
          <h2 className="text-[26px] leading-8 font-semibold tracking-[-0.015em]"><Skeleton mask="Nothing recognises it: a crawl would collect 4,260+ addresses" /></h2>
        </div>
        <section className="rounded-[10px] bg-canvas py-2 shadow-[inset_0_0_0_1px_var(--rule)]">
          <div className="border-b px-5 pt-2.5 pb-3 text-[11px] font-semibold uppercase tracking-[0.08em] text-muted-foreground">What happened, in order</div>
          {Array.from({ length: 5 }, (_, i) => <div key={i} className="flex gap-4 px-5 py-3">
            <span className="w-12 shrink-0 font-mono text-xs leading-[22px]"><Skeleton mask="+0.00" /></span>
            <div className="grid min-w-0 flex-1 gap-1">
              <div className="flex items-center gap-2.5"><Skeleton className="h-[22px] w-9 rounded-[5px]" /><span className="font-mono text-[13px]"><Skeleton mask="GET /robots.txt" /></span><span className="font-mono text-xs"><Skeleton mask="text/plain · 1.2 KB · 88 ms" /></span></div>
              <p className="text-[13px] leading-[19px]"><Skeleton mask="Read. Names no sitemap." /></p>
            </div>
          </div>)}
        </section>
      </div>
      <aside className="grid gap-6">
        <section>
          <SectionRule>At a glance</SectionRule>
          {['Recognised by', 'A crawl would find', 'Text on the front page', 'robots.txt', 'Links off-site'].map(label => <div key={label} className="flex justify-between gap-3 border-b py-2.5 text-[13px]"><span className="text-muted-foreground">{label}</span><span><Skeleton mask="4,260+ addresses" /></span></div>)}
        </section>
        <section className="grid gap-2.5">
          <SectionRule>The front page, as extracted</SectionRule>
          <Skeleton className="h-40 rounded-lg" />
        </section>
      </aside>
    </div>
  </>
}
