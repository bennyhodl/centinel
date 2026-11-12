import { describe, expect, it } from 'vitest'

import type { JobEvent, JobItem, JobState } from './api'
import { activeJobs, activeParts, foldJob, jobTitle, partStage } from './job-state'

let seq = 0
const at = 1_760_000_000_000
const event = (job: string, body: Record<string, unknown>) => ({ seq: ++seq, at: at + seq, job, ...body }) as JobEvent
const page = (address: string, verdict: JobItem['verdict']): JobItem => ({ address, tag: '200', verdict, noun: 'requests', bytes: 10, millis: 5 })
const fold = (events: JobEvent[], from: JobState[] = []) => events.reduce(foldJob, from)

describe('foldJob', () => {
  // The same events as `a_job_in_parts_keeps_each_parts_stage_and_count` in `jobs.rs`: the two folds must agree.
  it('follows a run over several sources source by source', () => {
    const [job] = fold([
      event('run-2', { type: 'started', kind: 'run', label: 'tampa, orlando' }),
      event('run-2', { part: 'tampa', type: 'step', step: 'tampa · collect' }),
      event('run-2', { part: 'orlando', type: 'step', step: 'orlando · discover' }),
      event('run-2', { part: 'tampa', type: 'progress', message: '1 stored, 0 failed', done: 1, total: 10, current: 'https://tampa.gov/a' }),
      event('run-2', { part: 'tampa', type: 'item', item: page('https://tampa.gov/a', 'ok') }),
      event('run-2', { part: 'orlando', type: 'item', item: page('https://orlando.gov/x', 'fail') }),
    ])
    // The job's own stage and count are left alone; every item still counts toward it.
    expect(job).toMatchObject({ ok: 1, failed: 1 })
    expect([job.step, job.done, job.total]).toEqual([undefined, undefined, undefined])
    expect(job.parts).toEqual([
      { name: 'tampa', step: 'tampa · collect', message: '1 stored, 0 failed', done: 1, total: 10, current: 'https://tampa.gov/a', ok: 1, failed: 0 },
      { name: 'orlando', step: 'orlando · discover', message: undefined, done: undefined, total: undefined, current: undefined, ok: 0, failed: 1 },
    ])
    expect(job.log.map(line => line.part)).toEqual([undefined, 'tampa', 'orlando', 'tampa', 'orlando'])
    expect(jobTitle(job)).toBe('Collecting 2 sources')
    expect(job.parts!.map(partStage)).toEqual(['Collecting', 'Discovering'])

    // A source says when its stages are over. One left walking is the one the job names.
    const [later] = fold([event('run-2', { part: 'tampa', type: 'step', step: 'tampa · done' })], [job])
    expect(activeParts(later).map(part => part.name)).toEqual(['orlando'])
    expect(partStage(later.parts![0])).toBe('Done')
    expect(jobTitle(later)).toBe('Discovering orlando')
    // The corpus-wide stages after them are the job's own.
    const [deriving] = fold([event('run-2', { type: 'step', step: 'extract' })], [later])
    expect(jobTitle(deriving)).toBe('Extracting')
  })

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

  it('carries whether a job can be stopped from its start', () => {
    const jobs = fold([
      event('2026-10-13T00:00:00Z', { type: 'started', kind: 'run', label: 'tampa', stoppable: true }),
      event('embed-1', { type: 'started', kind: 'embed', label: 'schedule' }),
    ])
    expect(jobs.map(job => Boolean(job.stoppable))).toEqual([true, false])
  })

  it('starts a new stage with no count of its own', () => {
    const jobs = fold([
      event('run-2', { type: 'started', kind: 'run', label: 'schedule' }),
      event('run-2', { type: 'progress', message: '1005 stored', done: 1005, total: 1005 }),
      event('run-2', { type: 'step', step: 'embed' }),
    ])
    expect(jobs[0]).toMatchObject({ step: 'embed', done: undefined, total: undefined })
  })

  it('keeps the twenty newest endings, as the server does, ties included', () => {
    // Twenty-one jobs that all end in one millisecond: the server keeps the twenty it
    // heard end last, and so must the page, or a snapshot would contradict it.
    const ended = Array.from({ length: 21 }, (_, n) => `job-${String(n).padStart(2, '0')}`)
    const started = fold(ended.map(id => event(id, { type: 'started', kind: 'embed', label: 'cli' })))
    const jobs = ended.reduce((all, id) => foldJob(all, { ...event(id, { type: 'finished', outcome: 'ok' }), at }), started)
    expect(jobs.map(job => job.id).sort()).toEqual(ended.slice(1))
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
