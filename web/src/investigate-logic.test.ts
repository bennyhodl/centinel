import { describe, expect, it } from 'vitest'

import type { InvestigateReport, InvestigationReport, RequestRecord } from './api'
import { agentReport, glanceOf, requestsOf, timelineOf, verdictOf } from './investigate-logic'

const t0 = Date.parse('2026-10-13T22:41:00Z')
const request = (offsetMs: number, kind: 'robots' | 'seed' | 'probe' | 'page', url: string, rest: Partial<RequestRecord> = {}): RequestRecord => ({
  step: kind === 'probe' ? { kind, strategy: 'sitemap' } : { kind },
  method: 'GET',
  url,
  status: 200,
  content_type: 'text/html',
  bytes: 2048,
  elapsed_ms: 90,
  at_ms: t0 + offsetMs,
  ...rest,
})

/** A site nothing recognises, whose front page reads as a menu and whose fallback crawl finds addresses. */
const unrecognised = (): InvestigationReport => ({
  address: 'https://x.gov/',
  started_at: '2026-10-13T22:41:00Z',
  finished_at: '2026-10-13T22:41:01.600Z',
  investigate: {
    report: {
      address: 'https://x.gov/',
      seed: { http_status: '200 OK', bytes: 94_000, kind: 'html', robots_declared: true },
      declined: ['listing', 'sitemap'],
      probe: { addresses: 500, complete: false, strategy: 'sitemap', by_fallback: true, requests_allowed: 25 },
      lead: { read: { chars: 695, bytes: 94_000, chars_per_kb: 7, links: 40, link_share: 0.84, findings: ['This is a menu, not a document.'] }, anchors: 40, script_bytes: 0, sitemap_declared: false },
      crumbs: [{ host: 'other.org', links: 3, pages: 1, example: 'https://other.org/' }],
      requests: [
        request(310, 'seed', 'https://x.gov/'),
        request(0, 'robots', 'https://x.gov/robots.txt', { content_type: 'text/plain' }),
        request(420, 'probe', 'https://x.gov/sitemap.xml', { status: 404, error: 'HTTP 404 Not Found', content_type: undefined, bytes: 0 }),
      ],
      elapsed_secs: 1.6,
    } as InvestigateReport,
  },
  check: {
    report: {
      target: 'https://x.gov/',
      documents: [{ address: 'https://x.gov/', bytes: 94_000, kind: 'html', chars: 695, tool: 'marked+htmd 0.5.5', preview: '# X\n\n[Home](/)' }],
      requests: [request(900, 'page', 'https://x.gov/')],
      elapsed_secs: 0.4,
    },
  },
})

const recognised = (): InvestigationReport => {
  const report = unrecognised()
  const investigate = report.investigate.report!
  investigate.recognised = [{ strategy: 'sitemap', keyed_on: 'sitemap.xml', kind: 'standard', best: true, evidence: [{ label: 'robots.txt', detail: 'declares one sitemap' }] }]
  investigate.declined = ['listing']
  investigate.probe = { addresses: 10, complete: true, strategy: 'sitemap', by_fallback: false, requests_allowed: 25 }
  investigate.lead = { ...investigate.lead!, read: { ...investigate.lead!.read, findings: [] }, sitemap_declared: true }
  investigate.requests = [
    request(0, 'robots', 'https://x.gov/robots.txt', { content_type: 'text/plain' }),
    request(310, 'seed', 'https://x.gov/'),
    request(420, 'probe', 'https://x.gov/sitemap.xml', { content_type: 'application/xml' }),
  ]
  return report
}

describe('verdictOf', () => {
  it('says what a crawl would collect when nothing recognises it, and rests on the front page that read as a menu', () => {
    const verdict = verdictOf(unrecognised())
    expect(verdict.headline).toBe('Nothing recognises it: a crawl would collect 500+ addresses')
    expect(verdict.tone).toBe('warn')
    expect(verdict.findings).toEqual(['This is a menu, not a document.'])
    expect(verdict.restsOn?.step.kind).toBe('seed')
  })

  it('names the method that recognised it, with an exact count when the probe finished, and rests on the walk', () => {
    const verdict = verdictOf(recognised())
    expect(verdict.headline).toBe('Sitemap recognises it: 10 addresses')
    expect(verdict.tone).toBe('ok')
    expect(verdict.restsOn?.url).toBe('https://x.gov/sitemap.xml')
  })

  it('does not call a site collectable when the crawl found nothing', () => {
    const report = unrecognised()
    report.investigate.report!.probe = { addresses: 0, complete: true, strategy: 'sitemap', by_fallback: true, requests_allowed: 25 }
    expect(verdictOf(report)).toMatchObject({ tone: 'bad', headline: 'Nothing recognises it, and a crawl found no addresses' })
  })

  it('reports a failed investigation as that, with the reason, rather than as a site that collects nothing', () => {
    const report = unrecognised()
    report.investigate = { error: 'timed out' }
    expect(verdictOf(report)).toMatchObject({ tone: 'bad', label: 'Investigation failed', headline: 'timed out' })
  })
})

describe('timelineOf', () => {
  it('orders the requests of both halves and puts the decisions where the recognisers ran', () => {
    const report = unrecognised()
    expect(requestsOf(report).map(r => r.url)).toEqual(['https://x.gov/robots.txt', 'https://x.gov/', 'https://x.gov/sitemap.xml', 'https://x.gov/'])

    const entries = timelineOf(report)
    expect(entries.map(entry => entry.type === 'request' ? entry.record.step.kind : 'decisions')).toEqual(['robots', 'seed', 'decisions', 'probe', 'page'])
    const first = entries[0]
    const seed = entries[1]
    if (first.type !== 'request' || seed.type !== 'request') throw new Error('expected requests')
    expect(first.offset).toBe(0)
    expect(seed.offset).toBeCloseTo(0.31)
  })

  it('says what a request meant only where a report says it', () => {
    const entries = timelineOf(unrecognised()).filter(entry => entry.type === 'request')
    const notes = entries.map(entry => entry.type === 'request' ? entry.note : undefined)
    expect(notes[0]).toBe('Read. Names no sitemap.')
    expect(notes[1]).toBe('Read as 695 characters, 84% of them links. This is a menu, not a document.')
    // The refused probe request carries its own error, then the walk's account of itself.
    expect(notes[2]).toBe('HTTP 404 Not Found. 500+ addresses found, as a fallback.')
    expect(notes[3]).toBe('Read by marked+htmd 0.5.5: 695 characters.')
  })

  it('marks exactly the request the verdict rests on', () => {
    const marked = timelineOf(unrecognised()).filter(entry => entry.type === 'request' && entry.restsOn)
    expect(marked).toHaveLength(1)
  })
})

describe('glanceOf', () => {
  it('reads the figures off the report', () => {
    const rows = Object.fromEntries(glanceOf(unrecognised()).map(row => [row.label, row.value]))
    expect(rows).toMatchObject({
      'Recognised by': 'nothing',
      'A crawl would find': '500+ addresses',
      'Text on the front page': '695 characters · poor',
      'robots.txt': 'read, names no sitemap',
      'Links off-site': '1 host',
    })
  })
})

describe('agentReport', () => {
  it('carries the verdict, every request, every method, the extraction and the raw reports', () => {
    const markdown = agentReport(unrecognised(), 'investigate-1')
    expect(markdown).toContain('# Centinel investigated https://x.gov/')
    expect(markdown).toContain('Nothing recognises it: a crawl would collect 500+ addresses.')
    expect(markdown).toContain('investigations/investigate-1/files')
    // A refused request, with the status the site gave and the reason.
    expect(markdown).toMatch(/\| 0\.42 \| probe \(sitemap\) \| 404 \| GET \| https:\/\/x\.gov\/sitemap\.xml \|/)
    expect(markdown).toContain('declined: `listing`')
    expect(markdown).toContain('no reason is recorded')
    expect(markdown).toContain('```text\n# X')
    expect(markdown).toContain('## Raw reports')
    expect(markdown).toContain('"at_ms"')
  })

  it('lists the method that recognised it with its evidence', () => {
    const markdown = agentReport(recognised(), 'investigate-2')
    expect(markdown).toContain('recognised: `sitemap` (would run), sitemap.xml (standard). robots.txt: declares one sitemap')
  })
})
