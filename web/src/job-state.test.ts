import { describe, expect, it } from 'vitest'

import type { JobEvent, JobItem, JobState } from './api'
import { activeJobs, foldJob, jobTitle } from './job-state'

let seq = 0
const at = 1_760_000_000_000
const event = (job: string, body: Record<string, unknown>) => ({ seq: ++seq, at: at + seq, job, ...body }) as JobEvent
const page = (address: string, verdict: JobItem['verdict']): JobItem => ({ address, tag: '200', verdict, noun: 'requests', bytes: 10, millis: 5 })
const fold = (events: JobEvent[], from: JobState[] = []) => events.reduce(foldJob, from)

describe('foldJob', () => {
  it('follows a collection from start to finish', () => {
    const jobs = fold([
      event('run-1', { type: 'started', kind: 'run', label: 'schedule' }),
      event('run-1', { type: 'step', step: 'tampa.gov · collect' }),
      event('run-1', { type: 'progress', message: '0 stored', done: 0, total: 1005, current: 'https://www.tampa.gov/a' }),
      event('run-1', { type: 'item', item: page('https://www.tampa.gov/a', 'ok') }),
      event('run-1', { type: 'progress', message: '1 stored', done: 1, total: 1005, current: 'https://www.tampa.gov/b' }),
      event('run-1', { type: 'item', item: page('https://www.tampa.gov/b', 'fail') }),
    ])
    const [job] = jobs
    expect(job).toMatchObject({ step: 'tampa.gov · collect', done: 1, total: 1005, current: 'https://www.tampa.gov/b', ok: 1, failed: 1 })
    expect(job.log.map(line => line.type)).toEqual(['started', 'step', 'item', 'item'])
    expect(activeJobs(jobs)).toHaveLength(1)

    const done = fold([event('run-1', { type: 'finished', outcome: 'ok' })], jobs)
    expect(done[0]).toMatchObject({ outcome: 'ok', current: undefined })
    expect(activeJobs(done)).toHaveLength(0)
  })

  it('starts a new stage with no count of its own', () => {
    const jobs = fold([
      event('run-2', { type: 'started', kind: 'run', label: 'schedule' }),
      event('run-2', { type: 'progress', message: '1005 stored', done: 1005, total: 1005 }),
      event('run-2', { type: 'step', step: 'embed' }),
    ])
    expect(jobs[0]).toMatchObject({ step: 'embed', done: undefined, total: undefined })
  })

  it('ignores an event the snapshot already holds', () => {
    const late = event('run-3', { type: 'item', item: page('https://x.gov/a', 'ok') })
    const snapshot: JobState = { id: 'run-3', kind: 'run', label: 'schedule', started_at: at, seq: late.seq, ok: 1, failed: 0, log: [late] }
    const jobs = fold([late], [snapshot])
    expect(jobs[0]).toBe(snapshot)
    expect(fold([event('run-9', { type: 'note', message: 'from a job this page never saw start' })], jobs)).toBe(jobs)
  })
})

describe('jobTitle', () => {
  it('says what a job is doing, and where', () => {
    expect(jobTitle({ kind: 'run', step: 'tampa.gov · collect' })).toBe('Collecting tampa.gov')
    expect(jobTitle({ kind: 'run', step: 'embed' })).toBe('Embedding')
    expect(jobTitle({ kind: 'classify' })).toBe('Classifying')
  })
})
