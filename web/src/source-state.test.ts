import { describe, expect, it } from 'vitest'

import type { LaneHolder, ListedSource } from './api'
import { nextAction, sourceState } from './source-state'

const source = (fields: Partial<ListedSource> = {}): ListedSource => ({
  id: 'tampa', kind: 'site', target: 'https://www.tampa.gov/', enabled: true, resources: 1005, liveness: { live: 990, gone: 15 }, ...fields,
})
const run = (outcome: NonNullable<ListedSource['last_run']>['outcome']) => ({ started_at: '2026-10-13T12:00:00Z', outcome, trigger: 'manual' as const, documents: 0 })
const lane = (sources: string[] = []): LaneHolder => ({ pid: 1, started_at: '2026-10-13T12:00:00Z', trigger: 'manual', sources, args: sources.join(' ') })

describe('sourceState', () => {
  it('is collecting while a run started for it, or for every enabled source, holds the lane', () => {
    expect(sourceState(source(), lane(['tampa']))).toBe('collecting')
    expect(sourceState(source(), lane())).toBe('collecting')
    expect(sourceState(source(), lane(['hillsborough']))).toBe('settled')
    expect(sourceState(source({ enabled: false }), lane())).toBe('disabled')
  })

  it('tells a source never collected from one that needs attention', () => {
    expect(sourceState(source({ resources: 0, liveness: {} }), null)).toBe('never')
    expect(sourceState(source({ last_run: run('failed') }), null)).toBe('attention')
    expect(sourceState(source({ last_run: run('partial') }), null)).toBe('attention')
    expect(sourceState(source({ liveness: { blocked: 40 } }), null)).toBe('attention')
    expect(sourceState(source({ last_run: run('interrupted') }), null)).toBe('settled')
  })
})

describe('nextAction', () => {
  it('offers the one thing to do next', () => {
    expect(nextAction('collecting', source())).toBe('stop')
    expect(nextAction('disabled', source({ enabled: false }))).toBe('enable')
    expect(nextAction('settled', source({ last_run: run('interrupted') }))).toBe('resume')
    expect(nextAction('never', source({ resources: 0 }))).toBe('run')
  })
})
