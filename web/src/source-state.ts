import type { ListedSource } from './api'

/** Where a source stands, as the Sources list sorts it into tabs. */
export type SourceState = 'collecting' | 'attention' | 'never' | 'disabled' | 'settled'

/** The one button a source's row and page offer. Resume and Run start the same run: every stage picks up where it stopped. */
export type NextAction = 'stop' | 'enable' | 'resume' | 'run'

type Source = Pick<ListedSource, 'enabled' | 'resources' | 'liveness' | 'collecting' | 'last_run'>

export function sourceState(source: Source): SourceState {
  if (source.collecting) return 'collecting'
  if (!source.enabled) return 'disabled'
  if (!source.last_run && !source.resources) return 'never'
  return troubled(source) ? 'attention' : 'settled'
}

/** Its last run failed or half-failed, or it collected pages and none of them is live. */
const troubled = (source: Source) => {
  const outcome = source.last_run?.outcome
  const collected = Object.values(source.liveness).reduce((sum, n) => sum + n, 0)
  return outcome === 'failed' || outcome === 'partial' || (collected > 0 && !source.liveness.live)
}

export function nextAction(state: SourceState, source: Source): NextAction {
  if (state === 'collecting') return 'stop'
  if (state === 'disabled') return 'enable'
  return runAction(source)
}

/** A source whose last run was stopped resumes; any other runs. */
export const runAction = (source: Pick<Source, 'last_run'>): 'resume' | 'run' => source.last_run?.outcome === 'interrupted' ? 'resume' : 'run'

/** The tabs over the list, in order. `settled` sources show under All only. */
export const TABS = [
  ['all', 'All'],
  ['collecting', 'Collecting'],
  ['attention', 'Needs attention'],
  ['never', 'Never collected'],
  ['disabled', 'Disabled'],
] as const satisfies ReadonlyArray<readonly [SourceState | 'all', string]>

export type Tab = (typeof TABS)[number][0]

export const inTab = (tab: Tab, state: SourceState) => tab === 'all' || tab === state
