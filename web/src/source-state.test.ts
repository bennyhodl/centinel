import { describe, expect, it } from 'vitest'

import type { ListedSource } from './api'
import { behind, needsYou, runAction, standing } from './source-state'

const done = { collected: 1005, read: 990, unreadable: 15, indexed: 990, embedded: 990 }
const source = (fields: Partial<ListedSource> = {}): ListedSource => ({
  id: 'tampa', kind: 'site', target: 'https://www.tampa.gov/', enabled: true, resources: 1005, liveness: { live: 990, gone: 15 }, collecting: false, stages: done, ...fields,
})
const run = (outcome: NonNullable<ListedSource['last_run']>['outcome']) => ({ started_at: '2026-10-13T12:00:00Z', outcome, trigger: 'manual' as const, documents: 0 })

describe('standing', () => {
  it('is done only when everything collected is read, indexed and embedded', () => {
    expect(standing(source())).toEqual({ mark: 'done', words: 'Up to date' })
    // A document nothing can read is not work left over.
    expect(behind(done)).toEqual({ read: 0, indexed: 0, embedded: 0 })
  })

  it('names the first stage that is behind, and how much waits on it', () => {
    expect(standing(source({ stages: { ...done, read: 900 } }))).toEqual({ mark: 'waiting', words: 'Not read yet · 90 waiting' })
    expect(standing(source({ stages: { ...done, indexed: 400 } }))).toEqual({ mark: 'waiting', words: 'Not indexed yet · 590 waiting' })
    expect(standing(source({ stages: { ...done, embedded: 0 } }))).toEqual({ mark: 'waiting', words: 'Not embedded yet' })
    expect(standing(source({ stages: { ...done, embedded: 660 } }))).toEqual({ mark: 'waiting', words: 'Embedded 66% · 330 waiting' })
  })

  it('does not call a source behind on embedding in a store that has never embedded anything', () => {
    expect(standing(source({ stages: { ...done, embedded: undefined } })).mark).toBe('done')
  })

  // Which sources the lane's run walks is the server's call (`Holder::covers`); a run named
  // for a paused source still collects it.
  it('says what the run is doing with a source it walks, whatever else is true of it', () => {
    const several = { parts: [{ name: 'tampa', step: 'tampa · collect', done: 412, total: 1682, ok: 412, failed: 0 }, { name: 'orlando', step: 'orlando · discover', ok: 0, failed: 0 }] }
    expect(standing(source({ collecting: true }), several)).toEqual({ mark: 'working', words: 'Collecting', count: '412 / 1,682' })
    expect(standing(source({ id: 'orlando', collecting: true, enabled: false, last_run: run('failed') }), several)).toEqual({ mark: 'working', words: 'Discovering', count: undefined })
    const first = { parts: [{ name: 'tampa', step: 'tampa · done', ok: 1005, failed: 0 }, several.parts[1]] }
    expect(standing(source({ collecting: true }), first)).toEqual({ mark: 'working', words: 'Collected · waiting on the rest of the run' })
    // One source alone is the whole job; so is a stage the run does for every source at once.
    expect(standing(source({ collecting: true }), { step: 'tampa · collect', done: 3, total: 10 })).toEqual({ mark: 'working', words: 'Collecting', count: '3 / 10' })
    const embedding = { step: 'embed', done: 6602, total: 9702, parts: [{ name: 'tampa', step: 'tampa · done', ok: 1005, failed: 0 }] }
    expect(standing(source({ collecting: true }), embedding)).toEqual({ mark: 'working', words: 'Embedding', count: '6,602 / 9,702' })
    // The lane is held and its job has not reached this page yet.
    expect(standing(source({ collecting: true }))).toEqual({ mark: 'working', words: 'Working', count: undefined })
  })

  it('tells stuck from waiting from paused', () => {
    expect(standing(source({ enabled: false }))).toEqual({ mark: 'paused', words: 'Paused' })
    expect(standing(source({ last_run: run('failed') }))).toEqual({ mark: 'blocked', words: 'The last run failed' })
    expect(standing(source({ liveness: { blocked: 40 } }))).toEqual({ mark: 'blocked', words: 'Blocked on 40 addresses' })
    expect(standing(source({ last_run: run('interrupted'), liveness: { live: 240 } }))).toEqual({ mark: 'waiting', words: 'Interrupted at 240 of 1,005' })
    expect(standing(source({ last_run: run('interrupted'), resources: 0, liveness: {} }))).toEqual({ mark: 'waiting', words: 'Interrupted before the first page' })
    expect(standing(source({ last_run: run('partial') })).mark).toBe('waiting')
    expect(standing(source({ resources: 0, liveness: {} }))).toEqual({ mark: 'waiting', words: 'Never collected' })
    expect(standing(source({ resources: 2000 }))).toEqual({ mark: 'waiting', words: '995 addresses not collected yet' })
  })

  it('counts waiting and stuck sources as needing the operator, and nothing else', () => {
    expect([standing(source({ last_run: run('failed') })), standing(source({ stages: { ...done, embedded: 0 } }))].every(needsYou)).toBe(true)
    expect([standing(source()), standing(source({ enabled: false })), standing(source({ collecting: true }))].some(needsYou)).toBe(false)
  })
})

describe('runAction', () => {
  it('resumes a source whose last run was stopped, and runs any other', () => {
    expect(runAction(source({ last_run: run('interrupted') }))).toBe('resume')
    expect(runAction(source({ last_run: run('ok') }))).toBe('run')
    expect(runAction(source())).toBe('run')
  })
})
