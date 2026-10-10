import { describe, expect, it } from 'vitest'
import type { SpendBucket, SpendPrice } from './api'
import { dayKey, isUnpriced, merge, priceOf, windowFor } from './spend-logic'

const bucket = (hour: string, model: string, patch: Partial<SpendBucket> = {}): SpendBucket => ({
  hour, model, provider: 'local', stage: 'embed', requests: 1, input_tokens: 100, output_tokens: 0, cost_usd: 0.000002, unpriced: 0, ...patch,
})

describe('the spend window', () => {
  it('lists every day of the window in the reader’s days, ending today', () => {
    const now = new Date(2026, 9, 10, 15, 30)
    const window = windowFor(7, now)
    expect(window.resolution).toBe('day')
    expect(window.periods).toEqual(['2026-10-04', '2026-10-05', '2026-10-06', '2026-10-07', '2026-10-08', '2026-10-09', '2026-10-10'])
    expect(new Date(window.since).getTime()).toBe(new Date(2026, 9, 4).getTime())
  })

  it('holds still within the day, so the server is not asked again for every render', () => {
    expect(windowFor(30, new Date(2026, 9, 10, 0, 1)).since).toBe(windowFor(30, new Date(2026, 9, 10, 23, 59)).since)
  })

  it('reads the past day by the hour', () => {
    const window = windowFor(1, new Date('2026-10-10T15:30:00Z'))
    expect(window.periods).toHaveLength(24)
    expect(window.periods.at(-1)).toBe('2026-10-10T15:00:00.000Z')
    expect(window.since).toBe('2026-10-09T16:00:00.000Z')
  })
})

describe('merging the ledger', () => {
  it('folds UTC hours into the reader’s days, one row per model, with empty days as zeros', () => {
    const window = windowFor(7, new Date(2026, 9, 10, 12))
    const day = (d: number, h: number) => new Date(2026, 9, d, h).toISOString()
    const merged = merge([
      bucket(day(9, 1), 'qwen3-embedding-4b'),
      bucket(day(9, 23), 'qwen3-embedding-4b'),
      bucket(day(10, 9), 'jev-1.13.0', { provider: 'jev', stage: 'classify', cost_usd: 0.000004, requests: 2, input_tokens: 50, output_tokens: 4 }),
    ], window)

    expect(merged.periods.map(p => p.period)).toEqual(window.periods)
    expect(merged.periods.find(p => p.period === '2026-10-09')?.byModel.get('qwen3-embedding-4b')?.tokens).toBe(200)
    expect(merged.periods.find(p => p.period === '2026-10-08')?.tokens).toBe(0)
    expect(merged.models.map(m => m.model).sort()).toEqual(['jev-1.13.0', 'qwen3-embedding-4b'])
    const jev = merged.models.find(m => m.model === 'jev-1.13.0')!
    expect([jev.requests, jev.inputTokens, jev.outputTokens, jev.tokens]).toEqual([2, 50, 4, 54])
    expect(jev.costShare).toBeCloseTo(0.5)
    expect(merged.byProvider.local.costUsd).toBeCloseTo(0.000004)
    expect(merged.byStage.classify.requests).toBe(2)
  })

  it('keeps a model with no price as unknown rather than free', () => {
    const window = windowFor(7, new Date(2026, 9, 10, 12))
    const merged = merge([bucket(new Date(2026, 9, 10, 1).toISOString(), 'qwen3-embedding-0.6b', { cost_usd: 0, unpriced: 1 })], window)
    expect(isUnpriced(merged.models[0])).toBe(true)
    expect(merged.models[0].tokens).toBe(100)
  })
})

describe('prices', () => {
  const prices: SpendPrice[] = [
    { model: 'jev-*', provider: 'jev', input: 0.042, output: 0, source: 'TypeSafe' },
    { model: 'qwen3-embedding-4b', provider: 'local', input: 0.02, output: 0, source: 'OpenRouter', priced_as: 'openrouter/qwen/qwen3-embedding-4b' },
  ]
  it('matches a Jev version to the Jev family and a model to its own line', () => {
    expect(priceOf('jev-1.13.0', prices)?.input).toBe(0.042)
    expect(priceOf('qwen3-embedding-4b', prices)?.priced_as).toBe('openrouter/qwen/qwen3-embedding-4b')
    expect(priceOf('qwen3-embedding-0.6b', prices)).toBeUndefined()
  })
})

it('names a local day by the reader’s calendar', () => {
  expect(dayKey(new Date(2026, 0, 5, 23, 59))).toBe('2026-01-05')
})
