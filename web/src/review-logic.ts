import type { Question, Review, ReviewCandidate } from './api'
import { isChoice } from './policy'

/** What a person answers with: `true`/`false` for a yes-or-no question, an option id for a choice. */
export type Answers = Record<string, boolean | string>

/** How near its threshold a score sits while it is still the model's guess, not its call. */
const MARGIN = 0.15

/** The question that can exclude: the junk gate. The first choice with an exclude option. */
export const gateOf = (questions: Question[]) =>
  questions.find(question => isChoice(question) && (question.options || []).some(option => option.action === 'exclude'))

/**
 * The model's answers for a card, in the person's vocabulary, so a card submitted
 * untouched records agreement: a choice's winning option; for a yes-or-no question,
 * whether its score reached the threshold, as the policy decides it. A question the
 * model never scored is absent.
 */
export function modelAnswers(questions: Question[], candidate: ReviewCandidate): Answers {
  const answers: Answers = {}
  for (const question of questions) {
    if (isChoice(question)) {
      const top = candidate.outcomes[question.id]?.top
      if (top) answers[question.id] = top
    } else {
      const score = candidate.classifications[question.id]
      if (score != null) answers[question.id] = score >= question.threshold
    }
  }
  return answers
}

/**
 * The score the policy acts on: a yes-or-no question's probability; for a choice, its
 * junk probability when it can exclude, else its winner's.
 */
export function decidingScore(question: Question, candidate: ReviewCandidate): number | undefined {
  if (!isChoice(question)) return candidate.classifications[question.id]
  const outcome = candidate.outcomes[question.id]
  if (!outcome?.top) return undefined
  return outcome.exclusion ?? candidate.classifications[`${question.id}:${outcome.top}`]
}

/** The policy held the answer for review, or its score sits near the threshold. */
export function unsure(question: Question, candidate: ReviewCandidate): boolean {
  const score = decidingScore(question, candidate)
  if (score == null) return false
  return Boolean(candidate.outcomes[question.id]?.review) || Math.abs(score - question.threshold) < MARGIN
}

/** The gate option a swipe means: the model's own when it already agrees, else the plain one. */
export function gateOption(gate: Question, top: string | undefined, keep: boolean): string {
  const options = gate.options || []
  const current = options.find(option => option.id === top)
  if (current && (current.action === 'exclude') === !keep) return current.id
  const fallback = keep ? ['record', 'service_info', 'other'] : ['navigation', 'unreadable', 'calendar_or_directory']
  return fallback.find(id => options.some(option => option.id === id))
    || options.find(option => (option.action === 'exclude') === !keep)?.id
    || options[0].id
}

/**
 * The card's answers with the junk call made. With a gate, its junk or keep option. Without
 * one, junk is a yes on the exclude question the model leaned to most, and keep is a no on
 * every exclude question. Undefined when no question can exclude.
 */
export function swipe(questions: Question[], candidate: ReviewCandidate, answers: Answers, keep: boolean): Answers | undefined {
  const gate = gateOf(questions)
  if (gate) return { ...answers, [gate.id]: gateOption(gate, candidate.outcomes[gate.id]?.top, keep) }
  const excluders = questions.filter(question => !isChoice(question) && question.action === 'exclude')
  if (!excluders.length) return undefined
  if (keep) return { ...answers, ...Object.fromEntries(excluders.map(question => [question.id, false])) }
  const score = (question: Question) => candidate.classifications[question.id] ?? -1
  const likeliest = excluders.reduce((best, question) => score(question) > score(best) ? question : best)
  return { ...answers, [likeliest.id]: true }
}

/** One review line from a card: every answer the person left or changed, beside the model's. */
export function buildReview(candidate: ReviewCandidate, questions: Question[], answers: Answers): Review {
  const review: Review = {
    source: candidate.source,
    resource: candidate.resource,
    derived_sha: candidate.derived_sha,
    verdicts: {},
  }
  for (const question of questions) {
    const human = answers[question.id]
    if (human == null) continue
    const model = isChoice(question) ? candidate.outcomes[question.id]?.top ?? null : candidate.classifications[question.id] ?? null
    review.verdicts[question.id] = { model, human }
  }
  return review
}
