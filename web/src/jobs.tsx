import { useEffect, useRef, useState } from 'react'
import { Link } from '@tanstack/react-router'
import { type InFlight, type RunResult } from './api'
import { number, seconds, tail } from './format'
import { answerSummary, useRunDetail } from './live'
import { decisionOf, decisionLabels, type Decision } from './policy'
import { DocumentLink } from './ui'
import { Sheet, SheetContent, SheetDescription, SheetTitle } from '@/components/ui/sheet'

/** One line of a job's log, stamped when this page first saw it. */
type Line = { key: string; at: number; kind: Decision | 'sent'; result?: RunResult; flight?: InFlight }

const tone: Record<Line['kind'], string> = { sent: 'text-[#B9AE98]', keep: 'text-[#8FB58A]', tag: 'text-[#8FB58A]', review: 'text-[#E8A65A]', exclude: 'text-[#E8A65A]', error: 'text-[#E57D6E]' }
const word: Record<Line['kind'], string> = { ...decisionLabels, sent: 'sent' }
const clock = (ms: number) => new Date(ms).toTimeString().slice(0, 8)

/**
 * A classify run as it happens, line by line. The server keeps the documents out to
 * Jev and the latest answers; this drawer stamps each one as it first appears and keeps
 * them, so the log reads like a log for as long as the drawer is open.
 */
export function JobDrawer({ id, onClose }: { id: string; onClose: () => void }) {
  const detail = useRunDetail(id, { page: 1, page_size: 1 }, 700)
  const run = detail.data
  const view = run?.view
  const [lines, setLines] = useState<Line[]>([])
  const seen = useRef(new Set<string>())
  useEffect(() => { seen.current = new Set(); setLines([]) }, [id])
  useEffect(() => {
    if (!view) return
    const now = Date.now()
    const fresh: Line[] = []
    for (const flight of view.in_flight || []) {
      const key = `sent:${flight.source}:${flight.resource}:${flight.attempt}`
      if (!seen.current.has(key)) { seen.current.add(key); fresh.push({ key, at: flight.started_ms || now, kind: 'sent', flight }) }
    }
    for (const result of [...(view.recent || [])].reverse()) {
      const key = `done:${result.source}:${result.resource}:${result.derived_sha}`
      if (!seen.current.has(key)) { seen.current.add(key); fresh.push({ key, at: now, kind: decisionOf(result), result }) }
    }
    if (fresh.length) setLines(current => [...fresh.reverse(), ...current].slice(0, 500))
  }, [view])

  const questions = run ? (run.effective_questions?.length ? run.effective_questions : run.questions) : []
  const scored = view?.scored ?? 0
  const total = run?.document_count ?? 0
  const totals = view?.documents
  const running = !run || run.status === 'running'
  return <Sheet open onOpenChange={open => !open && onClose()}>
    <SheetContent side="right" className="flex w-full flex-col gap-0 p-0 sm:max-w-[560px]">
      <div className="grid gap-3.5 border-b px-6 pt-6 pb-5">
        <span className={`inline-flex items-center gap-2 text-[11px] font-semibold uppercase tracking-[0.08em] ${running ? 'text-flame-ink' : 'text-muted-foreground'}`}>{running && <span className="size-2 rounded-full bg-flame shadow-[0_0_0_4px_#F6D9B4]" />}{running ? 'Classifying' : run?.status} · {run ? seconds((running ? Date.now() : Date.parse(run.completed_at || run.created_at)) - Date.parse(run.created_at)) : ''}</span>
        <SheetTitle className="font-serif text-[30px] leading-8 font-normal">Jev run</SheetTitle>
        <SheetDescription className="font-mono text-xs">{id}</SheetDescription>
        <span className="block h-1.5 rounded-full bg-[#EFE9DC]"><i className="block h-full rounded-full bg-foreground" style={{ width: `${total ? scored / total * 100 : 0}%` }} /></span>
        <div className="flex flex-wrap gap-6">
          <Count value={`${number(scored)} / ${number(total)}`} label="answered" />
          <Count value={number(totals?.kept ?? 0)} label="kept" />
          <Count value={number(totals?.tagged ?? 0)} label="tagged" tone="text-[#3E6539]" />
          <Count value={number(totals?.review ?? 0)} label="review" tone="text-flame-ink" />
          <Count value={number(totals?.excluded ?? 0)} label="exclude" />
          <Count value={number(totals?.errors ?? 0)} label="errors" tone="text-destructive" />
        </div>
      </div>
      <div className="flex min-h-0 flex-1 flex-col bg-[#1F1C17] py-3 font-mono text-xs leading-[22px]">
        <div className="flex justify-between px-5 pb-2 font-sans text-[11px] text-[#B9AE98]"><span>Live log · {running ? 'following' : 'finished'}</span><span>{number(view?.in_flight?.length || 0)} out to Jev</span></div>
        <div className="min-h-0 flex-1 overflow-y-auto">
          {lines.map(line => <div key={line.key} className="flex gap-3 px-5">
            <span className="w-16 shrink-0 text-[#6B6458]">{clock(line.at)}</span>
            <span className={`w-16 shrink-0 ${tone[line.kind]}`}>{word[line.kind]}</span>
            {line.result
              ? <span className="flex min-w-0 flex-1 gap-3"><DocumentLink doc={line.result} className="min-w-0 truncate text-[#F4EEE1] hover:underline">{tail(line.result.resource)}</DocumentLink><span className="min-w-0 flex-1 truncate text-[#B9AE98]">{answerSummary(questions, line.result)}</span></span>
              : <span className="min-w-0 flex-1 truncate text-[#B9AE98]">{line.flight && `${tail(line.flight.resource)}${line.flight.attempt > 1 ? ` · attempt ${line.flight.attempt}` : ''}`}</span>}
          </div>)}
          {!lines.length && <div className="px-5 text-[#B9AE98]">{running ? 'Waiting for the first answers…' : 'This run has finished. Open it for every result.'}</div>}
        </div>
      </div>
      <div className="flex items-center gap-3 border-t px-6 py-4">
        <span className="flex-1 text-[13px] text-muted-foreground">{run?.model} · {number(run?.questions.length ?? 0)} questions</span>
        <Link to="/runs" search={{ run: id, page: 1, outcome: '' }} onClick={onClose} className="inline-flex h-[34px] items-center rounded-lg px-3.5 text-[13px] font-medium shadow-[inset_0_0_0_1px_#CFC6B5]">Open run</Link>
      </div>
    </SheetContent>
  </Sheet>
}

function Count({ value, label, tone = '' }: { value: string; label: string; tone?: string }) {
  return <span className="grid gap-0.5"><b className={`text-lg leading-[22px] font-semibold ${tone}`}>{value}</b><span className="text-xs text-muted-foreground">{label}</span></span>
}
