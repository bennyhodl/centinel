import { describe, expect, it } from 'vitest'

import type { Question, ReviewCandidate } from './api'
import { buildReview, gateOf, gateOption, modelAnswers } from './review-logic'

const gate: Question = {
  id: 'page_kind', kind: 'choice', version: 1, threshold: 0.9, review: 0.5, action: 'tag',
  instructions: 'What is this text mainly?',
  options: [
    { id: 'record', description: 'A record.', action: 'keep' },
    { id: 'navigation', description: 'Menus.', action: 'exclude' },
    { id: 'unreadable', description: 'Noise.', action: 'exclude' },
    { id: 'other', description: 'None fits.', action: 'keep' },
  ],
}
const kind: Question = {
  id: 'record_type', kind: 'choice', version: 1, threshold: 0.75, review: 0.5, action: 'tag',
  instructions: 'What kind?',
  options: [
    { id: 'minutes', description: 'Minutes.', action: 'tag' },
    { id: 'agenda', description: 'An agenda.', action: 'tag' },
    { id: 'other', description: 'None fits.', action: 'keep' },
  ],
}
const laws: Question = { id: 'laws', kind: 'noul', version: 1, threshold: 0.8, review: 0.5, action: 'tag', instructions: 'A law?' }
const budget: Question = { ...laws, id: 'budget' }
const unscored: Question = { ...laws, id: 'housing' }
const questions = [gate, kind, laws, budget, unscored]

const candidate: ReviewCandidate = {
  source: 'city', resource: 'https://example.gov/minutes', derived_sha: 'd'.repeat(64), blob_sha: 'b'.repeat(64),
  observed_at: '2026-10-07T12:00:00Z', tool: 'test', chars: 900, chunks: 2, excluded: false,
  classifications: { 'page_kind:record': 0.93, 'page_kind:navigation': 0.07, 'record_type:minutes': 0.81, 'record_type:agenda': 0.19, laws: 0.9, budget: 0.3 },
  outcomes: {
    page_kind: { excluded: false, review: false, top: 'record', exclusion: 0.07 },
    record_type: { excluded: false, review: false, top: 'minutes', tags: ['minutes'] },
    laws: { excluded: false, review: false, tags: ['laws'] },
    budget: { excluded: false, review: false },
  },
  review_band: false,
  reviewed: false,
}

describe('the model’s answers in the person’s vocabulary', () => {
  it('names a choice’s winner and whether a tag was on, and skips the unscored', () => {
    expect(modelAnswers(questions, candidate)).toEqual({ page_kind: 'record', record_type: 'minutes', laws: true, budget: false })
  })
})

describe('swiping the gate', () => {
  it('finds the gate and keeps the model’s option when it already agrees', () => {
    expect(gateOf(questions)?.id).toBe('page_kind')
    expect(gateOption(gate, 'record', true)).toBe('record')
    expect(gateOption(gate, 'navigation', false)).toBe('navigation')
  })
  it('crosses to the plain option when the swipe disagrees', () => {
    expect(gateOption(gate, 'navigation', true)).toBe('record')
    expect(gateOption(gate, 'record', false)).toBe('navigation')
    expect(gateOption(gate, undefined, false)).toBe('navigation')
  })
})

describe('the review line', () => {
  it('carries every answer beside the model’s, trims the proposals, and names the document', () => {
    const answers = { ...modelAnswers(questions, candidate), laws: false, page_kind: 'navigation' }
    const review = buildReview(candidate, questions, answers, ' Ordinance Amendment , , fee schedule', '  menus around it ')
    expect(review.source).toBe('city')
    expect(review.derived_sha).toBe(candidate.derived_sha)
    expect(review.verdicts.page_kind).toEqual({ model: 'record', human: 'navigation' })
    expect(review.verdicts.laws).toEqual({ model: 0.9, human: false })
    expect(review.verdicts.budget).toEqual({ model: 0.3, human: false })
    expect(review.verdicts.housing).toBeUndefined()
    expect(review.proposed).toEqual(['Ordinance Amendment', 'fee schedule'])
    expect(review.note).toBe('menus around it')
  })
})
