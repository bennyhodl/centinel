import { describe, expect, it } from 'vitest'

import type { Question } from './api'
import { classificationBadges, decisionOf, estimateRun, optionScores, policyShort, reachOf, sortKeys } from './policy'

/**
 * The shapes the shipped defaults take, small enough to read here. The defaults
 * themselves live in core and are tested there; these only have to be the two kinds.
 */
const junkGate: Question = {
  id: 'page_kind', kind: 'choice', version: 1, threshold: 0.9, review: 0.5, action: 'tag',
  instructions: 'What is this text mainly?',
  options: [
    { id: 'record', description: 'A record.', action: 'keep' },
    { id: 'navigation', description: 'Menus.', action: 'exclude' },
    { id: 'calendar_or_directory', description: 'A listing.', action: 'exclude' },
    { id: 'unreadable', description: 'Noise.', action: 'exclude' },
    { id: 'other', description: 'None of the other options fits.', action: 'keep' },
  ],
}
const laws: Question = { id: 'laws', kind: 'noul', version: 1, threshold: 0.8, review: 0.5, action: 'tag', instructions: 'Does `text` contain a law?' }
const budget: Question = { ...laws, id: 'budget', instructions: 'Does `text` concern public money?' }

describe('classification badges', () => {
  it('shows a choice by its winning option and a noul only above its threshold', () => {
    const badges = classificationBadges({
      'page_kind:navigation': 0.93,
      'page_kind:record': 0.05,
      page_kind: 0.95,
      laws: 0.91,
      budget: 0.4,
    }, [junkGate, laws, budget])
    expect(badges.map(badge => badge.text)).toEqual(['navigation 93', 'laws 91'])
    expect(badges[0].tone).toBe('warning')
    expect(badges[1].tone).toBe('success')
  })
})

describe('run estimate', () => {
  it('counts text and the questions sent with every document', () => {
    const { tokens, cost } = estimateRun(4_000_000, 1_000, [], 0.042)
    expect(tokens).toBe(1_000_000)
    expect(cost).toBeCloseTo(0.042)
    expect(estimateRun(4_000_000, 1_000, [junkGate], 0.042).tokens).toBeGreaterThan(tokens)
  })

  it('caps a long document at the sampling limit', () => {
    expect(estimateRun(1_000_000, 1, [], 1, 80_000).tokens).toBe(20_000)
  })

  it('prices a follow-up for the share of documents its parent leads to it', () => {
    const lawsOnRecords: Question = { ...laws, when: 'page_kind:record' }
    const chain = [junkGate, lawsOnRecords]
    // A quarter of the scored documents put `record` at even odds or better.
    const scores = { 'page_kind:record': [10, 10, 10, 0, 0, 2, 2, 3, 0, 3] }
    expect(reachOf(junkGate, chain, scores)).toBe(1)
    expect(reachOf(lawsOnRecords, chain, scores)).toBe(0.25)
    expect(reachOf(lawsOnRecords, chain, {}), 'nothing scored yet: every document').toBe(1)

    const reach = (question: Question) => reachOf(question, chain, scores)
    const gateAlone = estimateRun(4_000_000, 1_000, [junkGate], 1).tokens
    const unknown = estimateRun(4_000_000, 1_000, chain, 1).tokens
    const quarter = estimateRun(4_000_000, 1_000, chain, 1, 80_000, reach).tokens
    // Each follow-up request carries the text again: 1,000 tokens a document, here.
    expect(unknown - gateAlone).toBeGreaterThan(1_000_000)
    expect(quarter - gateAlone).toBeCloseTo((unknown - gateAlone) / 4, -2)
  })
})

describe('decisions', () => {
  const base = { source: 's', resource: 'r', derived_sha: 'd', answers: {} }
  it('ranks exclude over review over tag over keep, and errors apart', () => {
    expect(decisionOf({ ...base, error: 'x' })).toBe('error')
    expect(decisionOf({ ...base, outcomes: { a: { excluded: true, review: false }, b: { excluded: false, review: true } } })).toBe('exclude')
    expect(decisionOf({ ...base, outcomes: { a: { excluded: false, review: true, tags: ['laws'] } } })).toBe('review')
    expect(decisionOf({ ...base, outcomes: { a: { excluded: false, review: false, tags: ['laws'] } } })).toBe('tag')
    expect(decisionOf({ ...base, outcomes: { a: { excluded: false, review: false } } })).toBe('keep')
  })

  it('lists choice options most likely first', () => {
    const scores = optionScores(junkGate, { ...base, answers: { 'page_kind:navigation': 0.7, 'page_kind:record': 0.3 } })
    expect(scores.slice(0, 2).map(entry => entry.option.id)).toEqual(['navigation', 'record'])
    expect(scores).toHaveLength(junkGate.options!.length)
  })
})

describe('labels', () => {
  it('say a policy in a few words', () => {
    expect(policyShort(junkGate)).toBe('3 junk kinds excluded at 0.90 · review from 0.50')
    expect(policyShort(laws)).toBe('Tag at 0.80 · review from 0.50')
  })

  it('offer every sortable key', () => {
    const keys = sortKeys([junkGate, laws]).map(([key]) => key)
    expect(keys.slice(0, 3)).toEqual(['decision', 'resource', 'page_kind'])
    expect(keys).toContain('page_kind:navigation')
    expect(keys).toContain('laws')
  })
})
