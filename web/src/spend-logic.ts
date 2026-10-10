import type { SpendBucket, SpendPrice, SpendProvider, SpendStage } from './api'

/**
 * The spend page's arithmetic, kept out of React. The server answers in UTC hours per
 * model; everything here folds those into the reader's own days and into one row per
 * model. Every model is its own series, local ones included.
 */

export type Metric = 'cost' | 'tokens'
export const WINDOWS = [1, 7, 30, 90] as const
export type WindowDays = (typeof WINDOWS)[number]

export type Window = {
  /** RFC 3339, what the server reads from. */
  since: string
  /** One key per period, oldest first: an hour's ISO start, or a local `YYYY-MM-DD`. */
  periods: string[]
  resolution: 'hour' | 'day'
}

const HOUR = 3_600_000

/** A local calendar day, `YYYY-MM-DD`. */
export const dayKey = (at: Date) => `${at.getFullYear()}-${String(at.getMonth() + 1).padStart(2, '0')}-${String(at.getDate()).padStart(2, '0')}`

/**
 * The past 24 hours by the hour, or the past `days` days by the reader's day. `since` only
 * moves when a new hour or day starts, so it is a stable cache key.
 */
export function windowFor(days: WindowDays, now = new Date()): Window {
  if (days === 1) {
    const top = Math.floor(now.getTime() / HOUR) * HOUR
    const periods = Array.from({ length: 24 }, (_, i) => new Date(top - (23 - i) * HOUR).toISOString())
    return { since: periods[0], periods, resolution: 'hour' }
  }
  const start = new Date(now.getFullYear(), now.getMonth(), now.getDate() - (days - 1))
  const periods = Array.from({ length: days }, (_, i) => dayKey(new Date(start.getFullYear(), start.getMonth(), start.getDate() + i)))
  return { since: start.toISOString(), periods, resolution: 'day' }
}

/**
 * The period an hour falls in. Hours are UTC, so a zone off the hour by thirty minutes
 * files a call up to half an hour into the neighbouring day.
 */
export const periodOf = (hour: string, resolution: Window['resolution']) =>
  resolution === 'hour' ? new Date(hour).toISOString() : dayKey(new Date(hour))

export type Figures = { costUsd: number; tokens: number; requests: number; unpriced: number }
const zero = (): Figures => ({ costUsd: 0, tokens: 0, requests: 0, unpriced: 0 })
const add = (into: Figures, b: SpendBucket) => {
  into.costUsd += b.cost_usd
  into.tokens += b.input_tokens + b.output_tokens
  into.requests += b.requests
  into.unpriced += b.unpriced
}

export type ModelTotals = Figures & {
  model: string
  provider: SpendProvider
  inputTokens: number
  outputTokens: number
  byStage: Record<SpendStage, Figures>
  costShare: number
  tokenShare: number
}

export type PeriodTotals = Figures & { period: string; byModel: Map<string, Figures> }

export type Merged = Figures & {
  models: ModelTotals[]
  /** Every period of the window, in order, with nothing in it as zeros. */
  periods: PeriodTotals[]
  byProvider: Record<SpendProvider, Figures>
  byStage: Record<SpendStage, Figures>
}

/** Folds hourly buckets into the window's periods and one row per model. */
export function merge(buckets: SpendBucket[], window: Window): Merged {
  const total = zero()
  const models = new Map<string, ModelTotals>()
  const periods = new Map(window.periods.map(period => [period, { period, ...zero(), byModel: new Map<string, Figures>() }]))
  const byProvider: Record<SpendProvider, Figures> = { jev: zero(), open_router: zero(), local: zero() }
  const byStage: Record<SpendStage, Figures> = { classify: zero(), embed: zero(), query: zero() }

  for (const bucket of buckets) {
    const period = periods.get(periodOf(bucket.hour, window.resolution))
    // An hour before the window's first local day: the server reads from `since`, so
    // this is only the half-hour-zone case above.
    if (!period) continue
    add(total, bucket)
    add(period, bucket)
    if (!period.byModel.has(bucket.model)) period.byModel.set(bucket.model, zero())
    add(period.byModel.get(bucket.model)!, bucket)
    add(byProvider[bucket.provider], bucket)
    add(byStage[bucket.stage], bucket)
    let model = models.get(bucket.model)
    if (!model) {
      model = { model: bucket.model, provider: bucket.provider, ...zero(), inputTokens: 0, outputTokens: 0, byStage: { classify: zero(), embed: zero(), query: zero() }, costShare: 0, tokenShare: 0 }
      models.set(bucket.model, model)
    }
    add(model, bucket)
    add(model.byStage[bucket.stage], bucket)
    model.inputTokens += bucket.input_tokens
    model.outputTokens += bucket.output_tokens
  }

  for (const model of models.values()) {
    model.costShare = total.costUsd > 0 ? model.costUsd / total.costUsd : 0
    model.tokenShare = total.tokens > 0 ? model.tokens / total.tokens : 0
  }
  return { ...total, models: [...models.values()], periods: [...periods.values()], byProvider, byStage }
}

/** Highest first by the metric shown, then by the other. */
export const sortModels = (models: ModelTotals[], metric: Metric) => [...models].sort((a, b) =>
  metric === 'cost' ? b.costUsd - a.costUsd || b.tokens - a.tokens : b.tokens - a.tokens || b.costUsd - a.costUsd)

/** A model nobody has a price for: its cost is unknown, not zero. */
export const isUnpriced = (m: Figures) => m.requests > 0 && m.unpriced >= m.requests

/** USD per million tokens, over the priced calls only. */
export const perMillion = (m: Figures) => isUnpriced(m) || m.tokens === 0 ? null : m.costUsd / m.tokens * 1_000_000

export const PROVIDERS: Record<SpendProvider, { label: string; hue: string }> = {
  jev: { label: 'Jev', hue: 'var(--flame)' },
  open_router: { label: 'OpenRouter', hue: 'var(--slate)' },
  local: { label: 'Local', hue: 'var(--moss)' },
}

export const STAGES: Record<SpendStage, string> = { classify: 'Classify', embed: 'Embed', query: 'Search' }

/**
 * A colour per model: its provider's hue, a lighter shade for each further model of the
 * same provider, in name order. Stable across windows, unlike an order by spend.
 */
export function modelColors(models: { model: string; provider: SpendProvider }[]) {
  const colors = new Map<string, string>()
  for (const provider of Object.keys(PROVIDERS) as SpendProvider[]) {
    const names = models.filter(m => m.provider === provider).map(m => m.model).sort()
    names.forEach((name, i) => colors.set(name, i === 0
      ? PROVIDERS[provider].hue
      : `color-mix(in oklab, ${PROVIDERS[provider].hue} ${Math.max(35, 100 - i * 22)}%, var(--background))`))
  }
  return colors
}

/** The price a model is charged at, matching a `jev-*` family as the server does. */
export const priceOf = (model: string, prices: SpendPrice[]) =>
  prices.find(p => p.model === model) ?? prices.find(p => p.model.endsWith('*') && model.startsWith(p.model.slice(0, -1)))

/** `openrouter/qwen/qwen3-embedding-8b` reads as `qwen/qwen3-embedding-8b` beside an OpenRouter mark. */
export const modelLabel = (model: string) => model.replace(/^openrouter\//, '')

const CURRENCY = new Intl.NumberFormat('en-US', { style: 'currency', currency: 'USD', minimumFractionDigits: 2, maximumFractionDigits: 2 })
const INTEGER = new Intl.NumberFormat('en-US')

/** Dollars, with enough places that a fraction of a cent does not read as nothing. */
export function formatUsd(value: number) {
  if (value > 0 && value < 0.01) return `$${value.toPrecision(2).replace(/0+$/, '')}`
  return CURRENCY.format(value)
}

/** Three significant figures and a suffix, so columns line up: `19.9B`, `76.7M`, `804K`. */
export function formatTokens(value: number) {
  const trim = (v: number) => v.toFixed(Math.abs(v) >= 100 ? 0 : Math.abs(v) >= 10 ? 1 : 2).replace(/\.0+$/, '')
  const abs = Math.abs(value)
  if (abs >= 1e12) return `${trim(value / 1e12)}T`
  if (abs >= 1e9) return `${trim(value / 1e9)}B`
  if (abs >= 1e6) return `${trim(value / 1e6)}M`
  if (abs >= 1e3) return `${trim(value / 1e3)}K`
  return INTEGER.format(Math.round(value))
}

export const formatCount = (value: number) => INTEGER.format(Math.round(value))

export function formatPercent(share: number) {
  const percent = share * 100
  return percent > 0 && percent < 0.1 ? '<0.1%' : `${percent.toFixed(1)}%`
}

const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec']

/** `2026-08-07` → `Aug 7`; an hour → `2 PM`. */
export function formatPeriod(period: string, resolution: Window['resolution']) {
  if (resolution === 'hour') return new Date(period).toLocaleTimeString('en-US', { hour: 'numeric' })
  const [, month, day] = period.split('-').map(Number)
  return `${MONTHS[month - 1]} ${day}`
}
