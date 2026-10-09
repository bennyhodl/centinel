import type { JobEvent, JobState } from './api'

/** The same bounds the server keeps: lines per job, finished jobs. */
const TAIL = 200
const FINISHED = 20

/**
 * Folds one live event into the jobs. The server folds the same events the same way
 * (`JobState::apply` in `crates/centinel-core/src/jobs.rs`); the two must agree. An event
 * the state already holds — the stream overlapping a snapshot — changes nothing.
 */
export function foldJob(jobs: JobState[], event: JobEvent): JobState[] {
  if (event.type === 'started') {
    if (jobs.some(job => job.id === event.job)) return jobs
    const job: JobState = { id: event.job, kind: event.kind, label: event.label, started_at: event.at, seq: event.seq, ok: 0, failed: 0, log: [event] }
    return [...jobs, job]
  }
  const at = jobs.findIndex(job => job.id === event.job)
  if (at < 0 || jobs[at].seq >= event.seq) return jobs
  const next = jobs.slice()
  next[at] = apply(jobs[at], event)
  return event.type === 'finished' ? trimFinished(next) : next
}

function apply(job: JobState, event: JobEvent): JobState {
  const log = event.type === 'progress' ? job.log : [...job.log, event].slice(-TAIL)
  const base = { ...job, seq: event.seq, log }
  switch (event.type) {
    case 'step':
      // A new stage counts a new work list.
      return { ...base, step: event.step, message: undefined, done: undefined, total: undefined, current: undefined }
    case 'progress':
      return { ...base, message: event.message, done: event.done, total: event.total, current: event.current }
    case 'item':
      return succeeded(event.item.verdict) ? { ...base, ok: job.ok + 1 } : { ...base, failed: job.failed + 1 }
    case 'finished':
      return { ...base, finished_at: event.at, outcome: event.outcome, error: event.error, current: undefined }
    default:
      return base
  }
}

function trimFinished(jobs: JobState[]): JobState[] {
  const finished = jobs.filter(job => job.outcome).sort((a, b) => (b.finished_at ?? 0) - (a.finished_at ?? 0))
  const kept = new Set(finished.slice(0, FINISHED).map(job => job.id))
  return jobs.filter(job => !job.outcome || kept.has(job.id))
}

export const succeeded = (verdict: string) => verdict === 'ok' || verdict === 'warn'

/** Running jobs in the order they started. */
export const activeJobs = (jobs: JobState[]) => jobs.filter(job => !job.outcome)

const verbs: Record<string, string> = {
  discover: 'Discovering', collect: 'Collecting', extract: 'Extracting', transcribe: 'Transcribing',
  index: 'Indexing', classify: 'Classifying', embed: 'Embedding', run: 'Running', done: 'Finishing',
}

/**
 * What a job is doing, in words: `Collecting tampa.gov`, `Embedding`. The stage comes from
 * the step `run` names (`tampa.gov · collect`), or from the job's own kind when it is one
 * stage on its own.
 */
export function jobTitle(job: Pick<JobState, 'kind' | 'step'>): string {
  const [first, second] = (job.step || job.kind).split(' · ')
  const [source, stage] = second === undefined ? ['', first] : [first, second]
  const verb = verbs[stage] || stage
  return source ? `${verb} ${source}` : verb
}
