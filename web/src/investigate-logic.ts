import type { CheckedDocument, InvestigationReport, Note, RequestRecord } from './api'
import { number, plural } from './format'

/*
 * What an investigation says, read off its two reports and nothing else. The page and the
 * Markdown an agent is handed both draw from here, so a sentence the page shows is a
 * sentence the agent reads, and neither invents one the reports do not carry.
 */

/** The requests of both halves, in the order they were sent. */
export function requestsOf(report: InvestigationReport): RequestRecord[] {
  const both = [...(report.investigate.report?.requests ?? []), ...(report.check.report?.requests ?? [])]
  return both.sort((a, b) => a.at_ms - b.at_ms)
}

export type Verdict = {
  tone: 'ok' | 'warn' | 'bad'
  /** Whether a method recognised it, in two or three words. */
  label: string
  headline: string
  /** What the reports found wrong, in their own words. */
  findings: string[]
  /** The request the verdict rests on, so the timeline can mark it. */
  restsOn?: RequestRecord
}

const sentence = (text: string) => text.charAt(0).toUpperCase() + text.slice(1)
const found = (count: number, complete: boolean) => `${number(count)}${complete ? '' : '+'} addresses`

export function verdictOf(report: InvestigationReport): Verdict {
  const { report: investigate, error } = report.investigate
  if (!investigate) return { tone: 'bad', label: 'Investigation failed', headline: error ?? 'Investigate returned nothing.', findings: [] }

  const best = investigate.recognised?.find(hit => hit.best)
  const probe = investigate.probe
  const findings = [...(investigate.lead?.read.findings ?? []), ...(investigate.lead?.findings ?? [])]
  const requests = requestsOf(report)

  const headline = best
    ? `${sentence(best.strategy)} recognises it${probe ? `: ${found(probe.addresses, probe.complete)}` : ''}`
    : !probe
      ? 'Nothing recognises it'
      : probe.addresses
        ? `Nothing recognises it: a crawl would collect ${found(probe.addresses, probe.complete)}`
        : 'Nothing recognises it, and a crawl found no addresses'
  const collects = best ? Boolean(!probe || probe.addresses) : Boolean(probe?.addresses)

  return {
    tone: !collects ? 'bad' : findings.length ? 'warn' : 'ok',
    label: best ? 'Recognised' : 'Not recognised',
    headline,
    findings,
    // A finding is about what the front page read as, so it rests on the seed; otherwise the
    // verdict rests on the walk that counted the addresses.
    restsOn: findings.length
      ? requests.find(record => record.step.kind === 'seed')
      : requests.find(record => record.step.kind === 'probe' && !record.error),
  }
}

export type Decisions = {
  recognised: Array<{ strategy: string; keyedOn: string; kind: string; best: boolean; evidence: Note[] }>
  declined: string[]
}

export type Entry =
  | { type: 'request'; record: RequestRecord; /** Seconds since the first request. */ offset: number; note?: string; restsOn: boolean }
  | { type: 'decisions'; decisions: Decisions }

/**
 * The requests in order, with every method's decision where it was made: recognisers run on
 * the seed and `robots.txt`, so the decisions follow the last of those two.
 */
export function timelineOf(report: InvestigationReport): Entry[] {
  const requests = requestsOf(report)
  const first = requests[0]?.at_ms ?? 0
  const { restsOn } = verdictOf(report)
  const investigate = report.investigate.report
  const decisions: Decisions | undefined = investigate && {
    recognised: (investigate.recognised ?? []).map(hit => ({ strategy: hit.strategy, keyedOn: hit.keyed_on, kind: hit.kind, best: hit.best, evidence: [...hit.evidence, ...(hit.warnings ?? [])] })),
    declined: investigate.declined ?? [],
  }
  const lastRead = requests.map(record => record.step.kind === 'robots' || record.step.kind === 'seed').lastIndexOf(true)

  const entries: Entry[] = requests.map(record => ({ type: 'request', record, offset: (record.at_ms - first) / 1000, note: noteFor(record, report), restsOn: record === restsOn }))
  if (decisions) entries.splice(lastRead + 1, 0, { type: 'decisions', decisions })
  return entries
}

/** What a request meant, where a report says. */
function noteFor(record: RequestRecord, report: InvestigationReport): string | undefined {
  const investigate = report.investigate.report
  const documents = report.check.report?.documents ?? []
  const parts: string[] = []
  if (record.error) parts.push(`${record.error}.`)
  if (record.redirected_to) parts.push(`Redirected to ${record.redirected_to}.`)

  switch (record.step.kind) {
    case 'robots':
      if (record.error) break
      parts.push(investigate?.lead?.sitemap_declared ? 'Read. Declares a sitemap.' : 'Read. Names no sitemap.')
      break
    case 'seed': {
      const read = investigate?.lead?.read
      if (read && !record.error) parts.push(`Read as ${number(read.chars)} characters, ${Math.round(read.link_share * 100)}% of them links.`, ...(read.findings ?? []))
      break
    }
    case 'probe': {
      // The walk's own account goes on its last request, which is where it was reached.
      const probe = investigate?.probe
      const lastProbe = requestsOf(report).filter(r => r.step.kind === 'probe').at(-1)
      if (probe && record === lastProbe) parts.push(`${found(probe.addresses, probe.complete)} found${probe.by_fallback ? ', as a fallback' : ''}.`)
      break
    }
    case 'page':
    case 'enclosure': {
      const doc = documents.find(d => d.address === record.url)
      if (doc) parts.push(doc.unextractable ?? `Read by ${doc.tool ?? 'no reader'}: ${number(doc.chars)} characters.`)
      break
    }
  }
  return parts.length ? parts.join(' ') : undefined
}

export type Glance = Array<{ label: string; value: string; tone?: 'bad' | 'warn' }>

export function glanceOf(report: InvestigationReport): Glance {
  const investigate = report.investigate.report
  if (!investigate) return []
  const best = investigate.recognised?.find(hit => hit.best)
  const read = investigate.lead?.read
  const robots = !investigate.seed.robots_declared ? 'unreachable, rules assumed' : investigate.lead?.sitemap_declared ? 'declares a sitemap' : 'read, names no sitemap'
  return [
    { label: 'Recognised by', value: best ? best.strategy : 'nothing', tone: best ? undefined : 'bad' },
    { label: 'A crawl would find', value: investigate.probe ? found(investigate.probe.addresses, investigate.probe.complete) : 'not probed' },
    ...(read ? [{ label: 'Text on the front page', value: `${number(read.chars)} characters${read.findings?.length ? ' · poor' : ''}`, tone: read.findings?.length ? 'bad' as const : undefined }] : []),
    { label: 'robots.txt', value: robots },
    { label: 'Links off-site', value: plural(investigate.crumbs?.length ?? 0, 'host') },
  ]
}

/** The page the investigation was pointed at: the first document `check` read that was not enclosed. */
export const frontPage = (report: InvestigationReport): CheckedDocument | undefined => report.check.report?.documents.find(doc => !doc.enclosed)

export const hostOf = (address: string) => {
  try { return new URL(address).host } catch { return address }
}

/** The path and query of an address, which is what a request line shows. */
export const pathOf = (address: string) => {
  try {
    const { pathname, search } = new URL(address)
    return `${pathname}${search}`
  } catch { return address }
}

export const sizeOf = (bytes: number) => bytes < 1024 ? `${bytes} B` : bytes < 1024 * 1024 ? `${(bytes / 1024).toFixed(1)} KB` : `${(bytes / 1024 / 1024).toFixed(1)} MB`

/** `text/html; charset=utf-8` as `text/html`. */
export const typeOf = (contentType?: string) => contentType?.split(';')[0].trim()

export const clock = (iso: string) => new Date(iso).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', hour12: false })

/** How long the investigation took, and how many requests it made, as the subtitle says it. */
export function summaryOf(report: InvestigationReport) {
  const seconds = Math.max(0, (Date.parse(report.finished_at) - Date.parse(report.started_at)) / 1000)
  return { requests: requestsOf(report).length, seconds }
}

const row = (cells: Array<string | number | undefined>) => `| ${cells.map(cell => String(cell ?? '').replaceAll('|', '\\|').replaceAll('\n', ' ')).join(' | ')} |`
const stepLabel = (record: RequestRecord) => record.step.kind === 'probe' ? `probe (${record.step.strategy})` : record.step.kind

/**
 * Everything an investigation found, as Markdown an agent can improve the scraping from: the
 * verdict, every request and what it returned, every method's decision, what extraction made
 * of the page, and last the raw reports. The bytes check saved are named, because an agent
 * working on a scraper wants the page as served.
 */
export function agentReport(report: InvestigationReport, id: string): string {
  const investigate = report.investigate.report
  const check = report.check.report
  const verdict = verdictOf(report)
  const { requests, seconds } = summaryOf(report)
  const entries = timelineOf(report)
  const lines = [
    `# Centinel investigated ${report.address}`,
    '',
    `Investigated ${report.started_at}: ${plural(requests, 'request')} in ${seconds.toFixed(1)} s. The bytes as served are on the server under \`investigations/${id}/files\` in the store.`,
    '',
    '## Verdict',
    '',
    verdict.headline + '.',
    ...verdict.findings.map(finding => `- ${finding}`),
  ]
  if (verdict.restsOn) lines.push('', `It rests on \`${verdict.restsOn.method} ${verdict.restsOn.url}\`.`)

  lines.push('', '## Every request, in order', '', row(['+s', 'step', 'status', 'method', 'url', 'type', 'bytes', 'ms', 'what it meant']), row(['---', '---', '---', '---', '---', '---', '---', '---', '---']))
  for (const entry of entries) {
    if (entry.type !== 'request') continue
    const { record } = entry
    lines.push(row([entry.offset.toFixed(2), stepLabel(record), record.status ?? 'no answer', record.method, record.url, typeOf(record.content_type), record.bytes, record.elapsed_ms, entry.note]))
  }

  lines.push('', '## Every method that was asked', '')
  if (report.investigate.error) lines.push(`investigate failed: ${report.investigate.error}`)
  for (const hit of investigate?.recognised ?? []) {
    lines.push(`- recognised: \`${hit.strategy}\`${hit.best ? ' (would run)' : ''}, ${hit.keyed_on} (${hit.kind}). ${[...hit.evidence, ...(hit.warnings ?? [])].map(note => `${note.label}: ${note.detail}`).join('; ')}`)
  }
  for (const name of investigate?.declined ?? []) lines.push(`- declined: \`${name}\`. Recognisers answer yes or no; no reason is recorded.`)
  if (investigate && !investigate.recognised?.length) lines.push('', 'Nothing recognised it.')
  if (investigate?.probe) {
    const { probe } = investigate
    lines.push('', `Probe: ${found(probe.addresses, probe.complete)} by \`${probe.strategy}\`${probe.by_fallback ? ' as a fallback' : ''}, ${probe.requests_allowed} requests allowed.`)
    for (const address of probe.sample ?? []) lines.push(`- ${address}`)
  }
  for (const warning of investigate?.warnings ?? []) lines.push(`- warning: ${warning}`)

  lines.push('', '## What extraction made of it', '')
  if (report.check.error) lines.push(`check failed: ${report.check.error}`, '')
  for (const doc of check?.documents ?? []) {
    lines.push(`### ${doc.address}${doc.enclosed ? ' (enclosed)' : ''}`, '', `${doc.http_status ?? '?'} · ${doc.declared_type ?? doc.kind} · ${doc.bytes} bytes · ${doc.tool ?? 'no reader'} · ${doc.chars} characters`)
    if (doc.unextractable) lines.push(`Unextractable: ${doc.unextractable}`)
    for (const note of [...(doc.verdict?.findings ?? []), ...(doc.notes ?? [])]) lines.push(`- ${note}`)
    if (doc.pages_needing_ocr) lines.push(`- ${doc.pages_needing_ocr} pages need OCR`)
    if (doc.preview) lines.push('', '```text', doc.preview, '```')
    lines.push('')
  }
  for (const address of check?.not_fetched ?? []) lines.push(`- named on the page but not fetched: ${address}`)

  if (investigate?.crumbs?.length) {
    lines.push('', '## Off-site links', '')
    for (const crumb of investigate.crumbs) lines.push(`- ${crumb.host}: ${plural(crumb.links, 'link')}`)
  }

  lines.push('', '## Raw reports', '', '```json', JSON.stringify({ investigate: report.investigate, check: report.check }, null, 2), '```')
  return lines.join('\n')
}
