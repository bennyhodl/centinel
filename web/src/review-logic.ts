import type { Question, Review, ReviewCandidate } from './api'
import { isChoice } from './policy'

/** What a person answers with: `true`/`false` for a yes-or-no question, an option id for a choice. */
export type Answers = Record<string, boolean | string>

/** The question that can exclude: the junk gate. The first choice with an exclude option. */
export const gateOf = (questions: Question[]) =>
  questions.find(question => isChoice(question) && (question.options || []).some(option => option.action === 'exclude'))

/**
 * The model's answers for a card, in the person's vocabulary, so a card submitted
 * untouched records agreement: a choice's winning option; for a yes-or-no question,
 * whether its tag was on. A question the model never scored is absent.
 */
export function modelAnswers(questions: Question[], candidate: ReviewCandidate): Answers {
  const answers: Answers = {}
  for (const question of questions) {
    const outcome = candidate.outcomes[question.id]
    if (isChoice(question)) {
      if (outcome?.top) answers[question.id] = outcome.top
    } else if (candidate.classifications[question.id] != null) {
      answers[question.id] = Boolean(outcome?.tags?.length)
    }
  }
  return answers
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

/** One review line from a card: every answer the person left or changed, beside the model's. */
export function buildReview(candidate: ReviewCandidate, questions: Question[], answers: Answers, proposed: string, note: string): Review {
  const review: Review = {
    source: candidate.source,
    resource: candidate.resource,
    derived_sha: candidate.derived_sha,
    verdicts: {},
    proposed: proposed.split(',').map(name => name.trim()).filter(Boolean),
    note: note.trim(),
  }
  for (const question of questions) {
    const human = answers[question.id]
    if (human == null) continue
    const model = isChoice(question) ? candidate.outcomes[question.id]?.top ?? null : candidate.classifications[question.id] ?? null
    review.verdicts[question.id] = { model, human }
  }
  return review
}
