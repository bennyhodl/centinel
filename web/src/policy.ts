import type { ChoiceOption, Question, QuestionAction, RunResult } from './api'

export const isChoice = (question: Question) => question.kind === 'choice'

export type ScoreBadge = { key: string; text: string; tone: 'default' | 'muted' | 'warning' | 'success' }

/**
 * Short labels for a Corpus row from its stored scores under the current questions: a
 * choice shows its winning option; a noul shows only when it passes its threshold.
 */
export function classificationBadges(scores: Record<string, number>, questions: Question[]): ScoreBadge[] {
  const badges: ScoreBadge[] = []
  for (const question of questions) {
    if (isChoice(question)) {
      let top: ChoiceOption | undefined
      let best = -1
      for (const option of question.options || []) {
        const score = scores[`${question.id}:${option.id}`]
        if (score != null && score > best) { best = score; top = option }
      }
      if (!top) continue
      const tone = top.action === 'exclude' ? 'warning' : top.action === 'tag' && best >= question.threshold ? 'success' : 'muted'
      badges.push({ key: question.id, text: `${top.id} ${Math.round(best * 100)}`, tone })
      continue
    }
    const score = scores[question.id]
    if (score == null || question.action === 'keep' || score < question.threshold) continue
    badges.push({ key: question.id, text: `${question.id} ${Math.round(score * 100)}`, tone: question.action === 'exclude' ? 'warning' : 'success' })
  }
  return badges
}

/** Whether a stored score set holds any current question at all. */
export const hasScores = (scores: Record<string, number>) => Object.keys(scores).length > 0

/**
 * A rough price before a run starts. English runs near four characters to a token.
 * Text above the sampling limit is counted at the limit. A run follows the chain, so a
 * question counts for the share of documents `reach` expects it to be asked of, and each
 * group of follow-ups sharing a `when` is a request of its own that carries the text
 * again. Mirrors the CLI's estimate in `ops/classify.rs`.
 */
export function estimateRun(chars: number, documents: number, questions: Question[], ratePerMillion: number, maxTextChars = 80_000, reach: (question: Question) => number = () => 1) {
  if (!documents) return { tokens: 0, cost: 0 }
  const averageChars = Math.min(chars / documents, maxTextChars)
  const questionChars = questions.reduce((sum, question) =>
    sum + reach(question) * (question.instructions.length + 70 + (question.options || []).reduce((total, option) => total + option.id.length + option.description.length, 0)), 0)
  const followUps = new Map(questions.flatMap(question => question.when ? [[question.when, reach(question)] as const] : []))
  const sends = 1 + [...followUps.values()].reduce((sum, share) => sum + share, 0)
  const tokens = Math.round(documents * (averageChars * sends + questionChars) / 4)
  return { tokens, cost: tokens * ratePerMillion / 1_000_000 }
}

/**
 * The share of documents a run is expected to ask `question` of, from the Search score
 * facets: one for a source; for a follow-up, its parent's share times the share of
 * documents scored on its tag that score 0.5 or more there. A tag nobody has scored yet
 * counts as every document, so the estimate errs high. The server prices the CLI's runs
 * the same way.
 */
export function reachOf(question: Question, questions: Question[], scores: Record<string, number[]> = {}): number {
  let share = 1
  let current: Question | undefined = question
  for (let step = 0; current?.when && step <= questions.length; step++) {
    const tag: string = current.when
    const tenths = scores[tag] || []
    const scored = tenths.reduce((sum, count) => sum + count, 0)
    if (scored) share *= tenths.slice(5).reduce((sum, count) => sum + count, 0) / scored
    current = questions.find(candidate => outcomesOf(candidate).some(branch => branch.tag === tag))
  }
  return share
}

/** The parts of a question the server versions or applies, for a dirty check. */
export const questionSnapshot = (questions: Question[]) => JSON.stringify(questions.map(question => ({
  id: question.id,
  kind: question.kind || 'noul',
  instructions: question.instructions,
  options: (question.options || []).map(option => [option.id, option.description, option.action]),
  threshold: question.threshold,
  review: question.review ?? null,
  action: question.action,
  when: question.when ?? null,
})))

/** What would stop the server accepting this question; empty when it would. */
export function questionProblem(question: Question): string {
  const id = /^[A-Za-z0-9_]+$/
  if (!id.test(question.id)) return 'Use only letters, numbers, and underscores in the id.'
  if (!question.instructions.trim()) return 'Write the question.'
  if (question.review != null && question.review > question.threshold) return 'The review floor must be at or below the threshold.'
  if (!isChoice(question)) return ''
  const options = question.options || []
  if (options.length < 2) return 'A choice needs at least two options.'
  if (options.length > 255) return 'A choice can have at most 255 options.'
  const seen = new Set<string>()
  for (const option of options) {
    if (!id.test(option.id)) return `Option “${option.id || '(empty)'}”: use only letters, numbers, and underscores.`
    if (seen.has(option.id)) return `Option “${option.id}” appears twice.`
    if (!option.description.trim()) return `Option “${option.id}” needs a description.`
    seen.add(option.id)
  }
  return ''
}

/** A choice with no `other` option forces a document that fits nothing into a wrong option. */
export const missingOther = (question: Question) =>
  isChoice(question) && !(question.options || []).some(option => option.id === 'other' || option.id === 'none')

export type Decision = 'exclude' | 'review' | 'tag' | 'keep' | 'not_asked' | 'error'

/** One word for what the current policy did with a document, strongest first. */
export function decisionOf(result: RunResult): Decision {
  if (result.error) return 'error'
  // Asked nothing: every question of the run was a follow-up its parent did not lead to.
  if (!Object.keys(result.answers || {}).length && !Object.keys(result.outcomes || {}).length) return 'not_asked'
  const outcomes = Object.values(result.outcomes || {})
  if (outcomes.some(outcome => outcome.excluded)) return 'exclude'
  if (outcomes.some(outcome => outcome.review)) return 'review'
  if (outcomes.some(outcome => outcome.tags?.length)) return 'tag'
  return 'keep'
}

export const decisionLabels: Record<Decision, string> = { exclude: 'Exclude', review: 'Review', tag: 'Tagged', keep: 'Keep', not_asked: 'Not asked', error: 'Error' }

/** Every tag the policy gave a document, across questions. */
export const tagsOf = (result: RunResult) => Object.values(result.outcomes || {}).flatMap(outcome => outcome.tags || [])

/** A choice's options with their probabilities for one document, most likely first. */
export function optionScores(question: Question, result: RunResult) {
  return (question.options || [])
    .map(option => ({ option, score: result.answers[`${question.id}:${option.id}`] ?? 0 }))
    .sort((a, b) => b.score - a.score)
}

/** A policy in a few words, for a collapsed question row. */
export function policyShort(question: Question): string {
  const at = question.threshold.toFixed(2)
  const band = question.review != null ? ` · review from ${question.review.toFixed(2)}` : ''
  if (!isChoice(question)) {
    if (question.action === 'keep') return 'Score only'
    return `${question.action === 'exclude' ? 'Exclude' : 'Tag'} at ${at}${band}`
  }
  const options = question.options || []
  const excluded = options.filter(option => option.action === 'exclude').length
  const tagged = options.filter(option => option.action === 'tag').length
  const parts = []
  if (excluded) parts.push(`${excluded} junk ${excluded === 1 ? 'kind' : 'kinds'} excluded at ${at}`)
  if (tagged) parts.push(`${tagged} ${tagged === 1 ? 'kind' : 'kinds'} tagged at ${at}`)
  return (parts.join(' · ') || 'Score only') + band
}

/** Every order the results table can take, as [server key, label]. */
export function sortKeys(questions: Question[]): Array<[string, string]> {
  const keys: Array<[string, string]> = [['decision', 'Decision'], ['resource', 'Document name']]
  for (const question of questions) {
    if (!isChoice(question)) { keys.push([question.id, question.id]); continue }
    const hasExclude = (question.options || []).some(option => option.action === 'exclude')
    keys.push([question.id, `${question.id} · ${hasExclude ? 'junk probability' : 'winning probability'}`])
    for (const option of question.options || []) keys.push([`${question.id}:${option.id}`, `${question.id} · ${option.id}`])
  }
  return keys
}

/** One answer a question can give, and the tag a follow-up hangs off. A noul's "no" tags nothing. */
export type Branch = { label: string; tag?: string; action: QuestionAction }

export function outcomesOf(question: Question): Branch[] {
  return isChoice(question)
    ? (question.options || []).map(option => ({ label: option.id, tag: `${question.id}:${option.id}`, action: option.action }))
    : [{ label: 'yes', tag: question.id, action: question.action }, { label: 'no', action: 'keep' }]
}

/** Jev's probability for one answer, from a document's stored answers. */
export function probabilityOf(question: Question, branch: Branch, answers: Record<string, number>) {
  if (isChoice(question)) return answers[`${question.id}:${branch.label}`]
  const yes = answers[question.id]
  return yes == null ? undefined : branch.label === 'yes' ? yes : 1 - yes
}

/** The answer Jev gave: a choice's likeliest option, or a noul's side of one half. */
export function answered(question: Question, answers?: Record<string, number>) {
  if (!answers) return undefined
  const scored = outcomesOf(question).map(branch => [branch.label, probabilityOf(question, branch, answers)] as const).filter(([, p]) => p != null)
  return scored.sort((a, b) => (b[1] ?? 0) - (a[1] ?? 0))[0]?.[0]
}
