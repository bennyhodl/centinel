import { describe, expect, it } from 'vitest'

import { classificationBadges, decisionOf, estimateRun, missingOther, optionScores, policyShort, questionProblem, sortKeys } from './policy'
import { cityTopics, junkGate, presets, recordType } from './presets'

describe('classification badges', () => {
  it('shows a choice by its winning option and a noul only above its threshold', () => {
    const badges = classificationBadges({
      'page_kind:navigation': 0.93,
      'page_kind:record': 0.05,
      page_kind: 0.95,
      laws: 0.91,
      budget: 0.4,
    }, [junkGate, ...cityTopics])
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
})

describe('presets', () => {
  it('are all valid questions with an escape option in every choice', () => {
    for (const preset of presets) {
      for (const question of preset.questions) {
        expect(questionProblem(question)).toBe('')
        expect(missingOther(question)).toBe(false)
      }
    }
  })

  it('exclude only junk kinds and tag only record kinds', () => {
    const excluded = junkGate.options!.filter(option => option.action === 'exclude').map(option => option.id)
    expect(excluded).toEqual(['navigation', 'calendar_or_directory', 'unreadable'])
    expect(recordType.options!.some(option => option.action === 'exclude')).toBe(false)
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
    expect(policyShort(cityTopics[0])).toBe('Tag at 0.80 · review from 0.50')
  })

  it('offer every sortable key', () => {
    const keys = sortKeys([junkGate, cityTopics[0]]).map(([key]) => key)
    expect(keys.slice(0, 3)).toEqual(['decision', 'resource', 'page_kind'])
    expect(keys).toContain('page_kind:navigation')
    expect(keys).toContain('laws')
  })
})
