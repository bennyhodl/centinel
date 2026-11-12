import type { JobPart, JobState, ListedSource, Stages } from './api'
import { partOver, stageWord } from './job-state'
import { number } from './format'

/**
 * The one thing a source's row says: it is being worked on, it is done, it is waiting on
 * the operator, it is stuck, or it is paused.
 */
export type Mark = 'working' | 'done' | 'waiting' | 'blocked' | 'paused'

/** A source's mark, the few words beside it, and a count when it is being worked on. */
export type Standing = { mark: Mark; words: string; count?: string }

type Source = Pick<ListedSource, 'id' | 'enabled' | 'resources' | 'liveness' | 'collecting' | 'last_run' | 'stages'>

/** What each stage has left to do, counted in documents. Nothing is behind when every number is zero. */
export function behind(stages: Stages) {
  return {
    read: Math.max(0, stages.collected - stages.read - stages.unreadable),
    indexed: Math.max(0, stages.read - stages.indexed),
    // A store with no vector table has never embedded anything, so nothing waits on it.
    embedded: stages.embedded === undefined ? 0 : Math.max(0, stages.indexed - stages.embedded),
  }
}

/**
 * Where a source stands. `job` is the run holding the lane, when one is: a source it
 * walks says what that run is doing with it, from its own part of the job when the run
 * walks several.
 */
export function standing(source: Source, job?: Pick<JobState, 'step' | 'done' | 'total' | 'parts'>): Standing {
  if (source.collecting) {
    const part = job?.parts?.find(part => part.name === source.id)
    // Its own collection is over and the run is still walking others; reading, indexing and embedding follow for all of them.
    if (part && partOver(part) && !job?.step) return { mark: 'working', words: 'Collected · waiting on the rest of the run' }
    const at: Pick<JobPart, 'step' | 'done' | 'total'> | undefined = part && !partOver(part) ? part : job?.step ? job : undefined
    return { mark: 'working', words: at?.step ? stageWord(at.step) : 'Working', count: at?.total ? `${number(at.done ?? 0)} / ${number(at.total)}` : undefined }
  }
  if (!source.enabled) return { mark: 'paused', words: 'Paused' }

  const { live = 0, blocked = 0 } = source.liveness
  const fetched = Object.values(source.liveness).reduce((sum, n) => sum + n, 0)
  const outcome = source.last_run?.outcome
  if (outcome === 'failed') return { mark: 'blocked', words: 'The last run failed' }
  if (fetched > 0 && !live) return { mark: 'blocked', words: blocked ? `Blocked on ${number(blocked)} addresses` : 'Nothing it collected is live' }

  if (outcome === 'interrupted') return { mark: 'waiting', words: fetched ? `Interrupted at ${number(fetched)} of ${number(source.resources)}` : 'Interrupted before the first page' }
  if (outcome === 'partial') return { mark: 'waiting', words: 'The last run finished part of the way' }
  if (!source.resources) return { mark: 'waiting', words: 'Never collected' }
  if (source.resources > fetched) return { mark: 'waiting', words: `${number(source.resources - fetched)} addresses not collected yet` }

  const left = behind(source.stages)
  if (left.read) return { mark: 'waiting', words: `Not read yet · ${number(left.read)} waiting` }
  if (left.indexed) return { mark: 'waiting', words: `Not indexed yet · ${number(left.indexed)} waiting` }
  if (left.embedded) {
    return { mark: 'waiting', words: source.stages.embedded ? `Embedded ${Math.floor(source.stages.embedded / source.stages.indexed * 100)}% · ${number(left.embedded)} waiting` : 'Not embedded yet' }
  }
  return { mark: 'done', words: 'Up to date' }
}

/** Whether a source is waiting on the operator: the list's one filter. */
export const needsYou = (standing: Standing) => standing.mark === 'waiting' || standing.mark === 'blocked'

/** A source whose last run was stopped resumes; any other runs. Both start the same run: every stage picks up where it stopped. */
export const runAction = (source: Pick<Source, 'last_run'>): 'resume' | 'run' => source.last_run?.outcome === 'interrupted' ? 'resume' : 'run'
