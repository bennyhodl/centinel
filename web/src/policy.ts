import type { ChoiceOption, Question, RunResult } from './api'

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
 * Every request also carries the questions, so their text is counted once per document.
 * Text above the sampling limit is counted at the limit.
 */
export function estimateRun(chars: number, documents: number, questions: Question[], ratePerMillion: number, maxTextChars = 80_000) {
  if (!documents) return { tokens: 0, cost: 0 }
  const averageChars = Math.min(chars / documents, maxTextChars)
  const questionChars = questions.reduce((sum, question) =>
    sum + question.instructions.length + 70 + (question.options || []).reduce((total, option) => total + option.id.length + option.description.length, 0), 0)
  const tokens = Math.round(documents * (averageChars + questionChars) / 4)
  return { tokens, cost: tokens * ratePerMillion / 1_000_000 }
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

export type Decision = 'exclude' | 'review' | 'tag' | 'keep' | 'error'

/** One word for what the current policy did with a document, strongest first. */
export function decisionOf(result: RunResult): Decision {
  if (result.error) return 'error'
  const outcomes = Object.values(result.outcomes || {})
  if (outcomes.some(outcome => outcome.excluded)) return 'exclude'
  if (outcomes.some(outcome => outcome.review)) return 'review'
  if (outcomes.some(outcome => outcome.tags?.length)) return 'tag'
  return 'keep'
}

export const decisionLabels: Record<Decision, string> = { exclude: 'Exclude', review: 'Review', tag: 'Tagged', keep: 'Keep', error: 'Error' }

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
