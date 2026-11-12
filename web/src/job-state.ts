import type { JobEvent, JobPart, JobState } from './api'

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
    const job: JobState = { id: event.job, kind: event.kind, label: event.label, stoppable: event.stoppable, started_at: event.at, seq: event.seq, ok: 0, failed: 0, log: [event] }
    return [...jobs, job]
  }
  const at = jobs.findIndex(job => job.id === event.job)
  if (at < 0 || jobs[at].seq >= event.seq) return jobs
  const next = jobs.slice()
  next[at] = apply(jobs[at], event)
  return event.type === 'finished' ? trimFinished(next) : next
}

/**
 * An event from a part moves that part: its stage and its count are its own, and the
 * job's are left to the stages the job runs as one. Its items still count toward the job.
 */
function apply(job: JobState, event: JobEvent): JobState {
  const log = event.type === 'progress' ? job.log : [...job.log, event].slice(-TAIL)
  const next: JobState = { ...job, seq: event.seq, log }
  if (event.part) {
    const name = event.part
    const parts = next.parts?.some(part => part.name === name) ? next.parts : [...(next.parts ?? []), { name, ok: 0, failed: 0 }]
    next.parts = parts.map(part => part.name === name ? moved(part, event) : part)
    if (event.type !== 'item') return next
  }
  const job_ = moved(next, event)
  return event.type === 'finished' ? { ...job_, finished_at: event.at, outcome: event.outcome, error: event.error, current: undefined } : job_
}

/** How far along a job or one of its parts is. Both move the same way. */
function moved<T extends Omit<JobPart, 'name'>>(standing: T, event: JobEvent): T {
  switch (event.type) {
    case 'step':
      // A new stage counts a new work list.
      return { ...standing, step: event.step, message: undefined, done: undefined, total: undefined, current: undefined }
    case 'progress':
      return { ...standing, message: event.message, done: event.done, total: event.total, current: event.current }
    case 'item':
      return succeeded(event.item.verdict) ? { ...standing, ok: standing.ok + 1 } : { ...standing, failed: standing.failed + 1 }
    default:
      return standing
  }
}

/**
 * Keeps the newest endings, as `Board::finish` does: by when each ended, and between two
 * that ended in the same millisecond, the one the server heard of last.
 */
function trimFinished(jobs: JobState[]): JobState[] {
  const finished = jobs.filter(job => job.outcome).sort((a, b) => (b.finished_at ?? 0) - (a.finished_at ?? 0) || b.seq - a.seq)
  const kept = new Set(finished.slice(0, FINISHED).map(job => job.id))
  return jobs.filter(job => !job.outcome || kept.has(job.id))
}

export const succeeded = (verdict: string) => verdict === 'ok' || verdict === 'warn'

/** Running jobs in the order they started. */
export const activeJobs = (jobs: JobState[]) => jobs.filter(job => !job.outcome)

const verbs: Record<string, string> = {
  ingest: 'Ingesting',
  discover: 'Discovering', collect: 'Collecting', extract: 'Extracting', transcribe: 'Transcribing',
  'vectors-verify': 'Verifying vectors', 'vectors-repair': 'Repairing vectors', 'vectors-embed': 'Embedding missing chunks',
  index: 'Indexing', classify: 'Classifying', embed: 'Embedding', run: 'Running', done: 'Finishing',
}

/** The stage a step names: `tampa.gov · collect` is `collect`, and so is a bare `collect`. */
const stageOf = (step: string) => step.split(' · ').at(-1)!

/** Whether a part's stages are over. `run` says so with a step of its own. */
export const partOver = (part: JobPart) => part.step !== undefined && stageOf(part.step) === 'done'

/** The parts of a job still under way. */
export const activeParts = (job: Pick<JobState, 'parts'>) => (job.parts ?? []).filter(part => !partOver(part))

/** A step's stage as a word: `Collecting` for `tampa.gov · collect`, `Embedding` for `embed`. */
export const stageWord = (step: string) => verbs[stageOf(step)] || stageOf(step)

/** What a part is doing, in a word: `Collecting`, `Discovering`, `Done`. */
export const partStage = (part: JobPart) => !part.step ? 'Waiting' : partOver(part) ? 'Done' : stageWord(part.step)

/**
 * What a job is doing, in words: `Collecting tampa.gov`, `Embedding`. The stage comes from
 * the step `run` names (`tampa.gov · collect`), or from the job's own kind when it is one
 * stage on its own. A run walking several sources at once has no one stage to name, so
 * it says how many it is collecting.
 */
export function jobTitle(job: Pick<JobState, 'kind' | 'step' | 'parts'>): string {
  const active = activeParts(job)
  if (!job.step && active.length > 1) return `Collecting ${active.length} sources`
  const [first, second] = (job.step || active[0]?.step || job.kind).split(' · ')
  const [source, stage] = second === undefined ? ['', first] : [first, second]
  const verb = verbs[stage] || stage
  return source ? `${verb} ${source}` : verb
}
