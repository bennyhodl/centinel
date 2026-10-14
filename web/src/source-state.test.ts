import { describe, expect, it } from 'vitest'

import type { ListedSource } from './api'
import { nextAction, sourceState } from './source-state'

const source = (fields: Partial<ListedSource> = {}): ListedSource => ({
  id: 'tampa', kind: 'site', target: 'https://www.tampa.gov/', enabled: true, resources: 1005, liveness: { live: 990, gone: 15 }, collecting: false, ...fields,
})
const run = (outcome: NonNullable<ListedSource['last_run']>['outcome']) => ({ started_at: '2026-10-13T12:00:00Z', outcome, trigger: 'manual' as const, documents: 0 })

describe('sourceState', () => {
  // Which sources the lane's run walks is the server's call (`Holder::covers`); a run named
  // for a disabled source still collects it.
  it('is collecting while the server says a run walks it, whatever else is true of it', () => {
    expect(sourceState(source({ collecting: true }))).toBe('collecting')
    expect(sourceState(source({ collecting: true, enabled: false, last_run: run('failed') }))).toBe('collecting')
    expect(sourceState(source())).toBe('settled')
    expect(sourceState(source({ enabled: false }))).toBe('disabled')
  })

  it('tells a source never collected from one that needs attention', () => {
    expect(sourceState(source({ resources: 0, liveness: {} }))).toBe('never')
    expect(sourceState(source({ last_run: run('failed') }))).toBe('attention')
    expect(sourceState(source({ last_run: run('partial') }))).toBe('attention')
    expect(sourceState(source({ liveness: { blocked: 40 } }))).toBe('attention')
    expect(sourceState(source({ last_run: run('interrupted') }))).toBe('settled')
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
