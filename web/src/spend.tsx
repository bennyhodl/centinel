import { useState, type ReactNode } from 'react'
import { useQuery } from '@tanstack/react-query'
import { useNavigate, useSearch } from '@tanstack/react-router'
import { cn } from 'cn'
import { RefreshCw } from 'lucide-react'
import type { SpendPrice, SpendStage } from './api'
import { queries } from './queries'
import { SpendChart } from './spend-chart'
import {
  formatCount, formatPercent, formatTokens, formatUsd, isUnpriced, merge, modelColors, modelLabel, perMillion, priceOf,
  PROVIDERS, sortModels, STAGES, windowFor, WINDOWS, type Figures, type Merged, type Metric, type ModelTotals, type Window, type WindowDays,
} from './spend-logic'
import { ErrorBox, PageHeader, Segmented } from './ui'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Skeleton } from '@/components/ui/skeleton'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'

/**
 * What every model call cost: Jev, OpenRouter, and the local models priced as the cloud
 * model with the same weights. T3 Code's usage page, with one series per model.
 */

export type SpendSearch = { metric: Metric; days: WindowDays }

export const spendSearch = (search: Record<string, unknown>): SpendSearch => ({
  metric: search.metric === 'tokens' ? 'tokens' : 'cost',
  days: WINDOWS.find(days => days === Number(search.days)) ?? 30,
})

const windowLabels: Array<[string, string]> = [['1', 'Past 24h'], ['7', '7 days'], ['30', '30 days'], ['90', '90 days']]

/** Mutes a figure while a new window is on its way. The delay keeps a quick answer from flashing. */
const figureClass = (loading: boolean) => cn('transition-opacity', loading && 'opacity-40 delay-150')

export function Spend() {
  const { metric, days } = useSearch({ from: '/spend' })
  const navigate = useNavigate({ from: '/spend' })
  const span = windowFor(days)
  const query = useQuery(queries.spend(span.since))
  const priceList = useQuery(queries.prices())
  const [breakdown, setBreakdown] = useState<'model' | 'time'>('model')
  const [open, setOpen] = useState('')
  const set = (next: Partial<SpendSearch>) => navigate({ search: previous => ({ ...previous, ...next }), replace: true })

  const loading = query.isPlaceholderData || query.isFetching
  const merged = merge(query.data?.buckets ?? [], span)
  const prices = priceList.data?.prices ?? []
  const models = sortModels(merged.models, metric)
  const colors = modelColors(models)
  const peak = Math.max(0, ...models.map(m => metric === 'cost' ? m.costUsd : m.tokens))
  const local = merged.byProvider.local
  const billed = merged.byProvider.jev.costUsd + merged.byProvider.open_router.costUsd
  const unpriced = models.filter(m => m.unpriced > 0)
  const selected = models.find(m => m.model === open)

  return <>
    <PageHeader title="Spend" detail="Every model call and what it cost. Local models are priced as the cloud.">
      <div className="flex flex-wrap items-center gap-2 lg:flex-nowrap">
        <Segmented small label="Metric" value={metric} onChange={value => set({ metric: value })} options={[['cost', 'Cost'], ['tokens', 'Tokens']]} />
        <Segmented small label="Window" value={String(days)} onChange={value => set({ days: Number(value) as WindowDays })} options={windowLabels} />
        <Button size="icon-sm" variant="ghost" aria-label="Refresh" onClick={() => void query.refetch()}><RefreshCw className={cn(query.isFetching && 'animate-spin motion-reduce:animate-none')} /></Button>
      </div>
    </PageHeader>
    {query.error ? <ErrorBox error={query.error} /> : !query.data ? <SpendSkeleton /> : <div className="flex max-w-5xl flex-col gap-8">
      <section className="grid gap-6 lg:grid-cols-[minmax(0,18rem)_minmax(0,1fr)]">
        <div className="flex min-w-0 flex-col gap-5">
          <div className="flex flex-col gap-1">
            <span className={cn('text-4xl font-semibold tabular-nums', figureClass(loading))}>{metric === 'cost' ? formatUsd(merged.costUsd) : formatTokens(merged.tokens)}</span>
            <span className="text-xs text-muted-foreground">
              <span className={figureClass(loading)}>{formatCount(merged.requests)} {merged.requests === 1 ? 'call' : 'calls'}</span>
              {metric === 'cost' && local.requests > 0 && ` · ${formatUsd(billed)} billed, ${formatUsd(local.costUsd)} local at cloud prices`}
            </span>
          </div>
          {models.map(model => <ModelRow key={model.model} model={model} metric={metric} color={colors.get(model.model)!} price={priceOf(model.model, prices)} loading={loading} />)}
          {!models.length && <p className="text-sm text-muted-foreground">No model calls in this window.</p>}
        </div>
        <div className="flex min-w-0 flex-col gap-3">
          <h2 className="text-sm font-medium">{span.resolution === 'hour' ? 'Hourly' : 'Daily'} {metric === 'tokens' ? 'tokens' : 'cost'}</h2>
          {/* A model with no price has no cost line: unknown is not zero. */}
          <div className={figureClass(loading)}><SpendChart models={models.filter(m => metric === 'tokens' || !isUnpriced(m)).map(m => m.model)} colors={colors} periods={merged.periods} metric={metric} resolution={span.resolution} /></div>
        </div>
      </section>

      <section className="flex flex-col gap-2">
        <h2 className="text-sm font-medium">Totals</h2>
        <div className="grid grid-cols-2 gap-x-6 gap-y-4 py-1 md:grid-cols-5">
          <Metric loading={loading} label="Tokens" value={formatTokens(merged.tokens)} />
          <Metric loading={loading} label="Calls" value={formatCount(merged.requests)} />
          <Metric loading={loading} label="Billed" value={formatUsd(billed)} />
          <Metric loading={loading} label="Local at cloud prices" value={formatUsd(local.costUsd)} />
          <Metric loading={loading} label="Unpriced tokens" value={formatTokens(unpriced.reduce((sum, m) => sum + m.tokens, 0))} />
        </div>
      </section>

      {merged.tokens > 0 && <section className={cn('grid gap-x-12 gap-y-8 lg:grid-cols-2', figureClass(loading))}>
        <ShareBar label={`${metric === 'cost' ? 'Cost' : 'Tokens'} by provider`} metric={metric} segments={(Object.keys(PROVIDERS) as Array<keyof typeof PROVIDERS>).map(p => ({ label: PROVIDERS[p].label, figures: merged.byProvider[p], color: PROVIDERS[p].hue }))} />
        <ShareBar label={`${metric === 'cost' ? 'Cost' : 'Tokens'} by stage`} metric={metric} segments={stageSegments(merged.byStage)} />
      </section>}

      <section className="flex flex-col gap-3">
        <div className="flex items-center justify-between gap-3">
          <h2 className="text-sm font-medium">Breakdown</h2>
          <Segmented small label="Breakdown" value={breakdown} onChange={setBreakdown} options={[['model', 'Model'], ['time', span.resolution === 'hour' ? 'Hour' : 'Day']]} />
        </div>
        {breakdown === 'model'
          ? <ModelTable models={models} metric={metric} colors={colors} peak={peak} loading={loading} onOpen={setOpen} />
          : <PeriodTable merged={merged} models={models} span={span} loading={loading} />}
      </section>
    </div>}
    {selected && <ModelDialog model={selected} metric={metric} color={colors.get(selected.model)!} price={priceOf(selected.model, prices)} span={span} buckets={query.data?.buckets ?? []} onClose={() => setOpen('')} />}
  </>
}

/** Which provider ran a model, and for a local one, whose price it takes. */
function providerLine(model: ModelTotals, price: SpendPrice | undefined) {
  if (model.provider !== 'local') return PROVIDERS[model.provider].label
  return price?.priced_as ? `Local · priced as ${modelLabel(price.priced_as)}` : 'Local · no cloud price'
}

function ModelRow({ model, metric, color, price, loading }: { model: ModelTotals; metric: Metric; color: string; price?: SpendPrice; loading: boolean }) {
  const unknown = isUnpriced(model)
  return <div className="flex flex-col gap-1">
    <div className="flex items-baseline justify-between gap-4">
      <span className="flex min-w-0 items-center gap-2 text-sm">
        <span aria-hidden className="size-2 shrink-0 rounded-full" style={{ backgroundColor: color }} />
        <span className="flex min-w-0 items-baseline gap-1.5"><span className="truncate">{modelLabel(model.model)}</span><span className={cn('shrink-0 text-[11px] whitespace-nowrap text-muted-foreground tabular-nums', figureClass(loading))}>{formatCount(model.requests)} {model.requests === 1 ? 'call' : 'calls'}</span></span>
      </span>
      <span className={cn('shrink-0 text-sm font-medium tabular-nums', figureClass(loading))}>{metric === 'tokens' ? formatTokens(model.tokens) : unknown ? 'Unpriced' : formatUsd(model.costUsd)}</span>
    </div>
    <span className={cn('text-xs text-muted-foreground', figureClass(loading))}>
      {providerLine(model, price)} · {metric === 'cost' ? `${unknown ? '—' : formatPercent(model.costShare)} of cost · ${formatTokens(model.tokens)} tokens` : `${formatPercent(model.tokenShare)} of tokens · ${unknown ? 'unpriced' : formatUsd(model.costUsd)}`}
    </span>
  </div>
}

function Metric({ label, value, loading }: { label: string; value: string; loading: boolean }) {
  return <div className="flex min-w-0 flex-col gap-0.5">
    <span className="text-xs text-muted-foreground">{label}</span>
    <span className={cn('text-base font-medium tabular-nums', figureClass(loading))}>{value}</span>
  </div>
}

type Segment = { label: string; figures: Figures; color: string }

/** Stages brighten from the corpus's bulk to the reader's own searches. */
const ink = (percent: number) => `color-mix(in oklab, var(--foreground) ${percent}%, var(--background))`
const stageSegments = (byStage: Record<SpendStage, Figures>): Segment[] => [
  { label: STAGES.classify, figures: byStage.classify, color: ink(100) },
  { label: STAGES.embed, figures: byStage.embed, color: ink(60) },
  { label: STAGES.query, figures: byStage.query, color: ink(30) },
]

/** One part-to-whole bar with its legend. Empty parts are left out; nothing renders without a total. */
function ShareBar({ label, segments, metric, aside }: { label: string; segments: Segment[]; metric: Metric; aside?: ReactNode }) {
  const valueOf = (s: Segment) => metric === 'cost' ? s.figures.costUsd : s.figures.tokens
  const format = metric === 'cost' ? formatUsd : formatTokens
  const visible = segments.filter(s => valueOf(s) > 0)
  const total = visible.reduce((sum, s) => sum + valueOf(s), 0)
  if (total <= 0) return null
  return <div className="flex min-w-0 flex-col gap-2.5">
    <div className="flex items-baseline justify-between gap-3"><h3 className="text-sm font-medium">{label}</h3>{aside}</div>
    <div role="img" aria-label={`${label}: ${visible.map(s => `${s.label} ${format(valueOf(s))}`).join(', ')}`} className="flex h-2 gap-0.5">
      {visible.map(s => <Tooltip key={s.label}>
        <TooltipTrigger asChild><div className="h-full min-w-1 rounded-xs first:rounded-l-full last:rounded-r-full" style={{ flex: `${valueOf(s)} 1 0`, backgroundColor: s.color }} /></TooltipTrigger>
        <TooltipContent>{s.label} · {format(valueOf(s))} · {formatPercent(valueOf(s) / total)}</TooltipContent>
      </Tooltip>)}
    </div>
    <div className="flex flex-wrap gap-x-4 gap-y-1 text-xs">
      {visible.map(s => <span key={s.label} className="flex items-center gap-1.5"><span aria-hidden className="size-2 rounded-xs" style={{ backgroundColor: s.color }} /><span className="text-muted-foreground">{s.label}</span><span className="tabular-nums">{format(valueOf(s))}</span></span>)}
    </div>
  </div>
}

function ModelTable({ models, metric, colors, peak, loading, onOpen }: { models: ModelTotals[]; metric: Metric; colors: Map<string, string>; peak: number; loading: boolean; onOpen: (model: string) => void }) {
  return <table className="w-full text-sm">
    <thead><tr className="border-b border-border text-right text-xs text-muted-foreground">
      <th className="py-2 pr-3 text-left font-normal">#</th><th className="w-full py-2 text-left font-normal">Model</th>
      <th className="py-2 pl-6 font-normal">Cost</th><th className="hidden py-2 pl-6 font-normal sm:table-cell">Share</th><th className="py-2 pl-6 font-normal">Tokens</th>
    </tr></thead>
    <tbody>
      {!models.length ? <tr><td colSpan={5} className="py-6 text-center text-muted-foreground">No activity in this window.</td></tr>
        : models.map((model, i) => {
          const value = metric === 'cost' ? model.costUsd : model.tokens
          const unknown = isUnpriced(model)
          const share = metric === 'tokens' ? model.tokenShare : unknown ? null : model.costShare
          return <tr key={model.model} className={cn('relative border-b border-border/50 text-right whitespace-nowrap text-muted-foreground tabular-nums transition-colors hover:bg-hover has-focus-visible:bg-hover', figureClass(loading))}>
            <td className="py-2.5 pr-3 text-left text-xs">{i + 1}</td>
            <td className="py-2.5 text-left whitespace-normal">
              {/* The overlay makes the whole row open the model. */}
              <button type="button" onClick={() => onOpen(model.model)} className="flex items-center gap-2 text-left text-foreground outline-none after:absolute after:inset-0">{modelLabel(model.model)}<span className="text-xs text-muted-foreground">{PROVIDERS[model.provider].label}</span></button>
              <div aria-hidden className="mt-1.5 h-0.5 max-w-48"><div className="h-full rounded-full" style={{ width: value > 0 && peak > 0 ? `max(0.5rem, ${value / peak * 100}%)` : 0, backgroundColor: colors.get(model.model) }} /></div>
            </td>
            <td className="py-2.5 pl-6 text-foreground">{unknown ? <span className="text-muted-foreground">Unpriced</span> : formatUsd(model.costUsd)}</td>
            <td className="hidden py-2.5 pl-6 sm:table-cell">{share === null ? '' : formatPercent(share)}</td>
            <td className="py-2.5 pl-6">{formatTokens(model.tokens)}</td>
          </tr>
        })}
    </tbody>
  </table>
}

function PeriodTable({ merged, models, span, loading }: { merged: Merged; models: ModelTotals[]; span: Window; loading: boolean }) {
  const rows = merged.periods.filter(p => p.requests > 0).reverse()
  const width = `${60 / (models.length + 2)}%`
  return <table className="w-full table-fixed text-sm">
    <colgroup><col className="w-2/5" />{models.map(m => <col key={m.model} style={{ width }} />)}<col style={{ width }} /><col style={{ width }} /></colgroup>
    <thead><tr className="border-b border-border text-left text-xs text-muted-foreground">
      <th className="py-2 font-normal">{span.resolution === 'hour' ? 'Hour' : 'Day'}</th>
      {models.map(m => <th key={m.model} className="truncate py-2 text-right font-normal" title={m.model}>{modelLabel(m.model)}</th>)}
      <th className="py-2 text-right font-normal">Total</th><th className="py-2 text-right font-normal">Tokens</th>
    </tr></thead>
    <tbody>
      {!rows.length ? <tr><td colSpan={models.length + 3} className="py-6 text-center text-muted-foreground">No activity in this window.</td></tr>
        : rows.map(period => <tr key={period.period} className={cn('border-b border-border/50 transition-colors hover:bg-hover', figureClass(loading))}>
          <td className="py-2">{span.resolution === 'hour' ? new Date(period.period).toLocaleString('en-US', { month: 'short', day: 'numeric', hour: 'numeric' }) : new Date(`${period.period}T12:00:00`).toLocaleDateString('en-US', { month: 'short', day: 'numeric', weekday: 'short' })}</td>
          {models.map(m => <td key={m.model} className="py-2 text-right text-muted-foreground tabular-nums">{formatUsd(period.byModel.get(m.model)?.costUsd ?? 0)}</td>)}
          <td className="py-2 text-right tabular-nums">{formatUsd(period.costUsd)}</td>
          <td className="py-2 text-right text-muted-foreground tabular-nums">{formatTokens(period.tokens)}</td>
        </tr>)}
    </tbody>
  </table>
}

/** One model in the current window: its figures, its line alone, where its spend went, and its price. */
function ModelDialog({ model, metric, color, price, span, buckets, onClose }: {
  model: ModelTotals; metric: Metric; color: string; price?: SpendPrice; span: Window; buckets: Parameters<typeof merge>[0]; onClose: () => void
}) {
  const alone = merge(buckets.filter(b => b.model === model.model), span)
  const unknown = isUnpriced(model)
  const rate = perMillion(model)
  const stats = [
    { label: 'Cost', value: unknown ? 'Unpriced' : formatUsd(model.costUsd) },
    { label: 'Tokens', value: formatTokens(model.tokens) },
    { label: 'Calls', value: formatCount(model.requests) },
    rate === null ? null : { label: 'Per 1M tokens', value: formatUsd(rate) },
  ].filter(stat => stat !== null)
  return <Dialog open onOpenChange={next => { if (!next) onClose() }}>
    <DialogContent className="sm:max-w-3xl">
      <DialogHeader>
        <DialogTitle className="flex items-center gap-2"><span aria-hidden className="size-2.5 rounded-full" style={{ backgroundColor: color }} />{modelLabel(model.model)}</DialogTitle>
        <DialogDescription>{providerLine(model, price)}{unknown ? '' : ` · ${formatPercent(model.costShare)} of cost`}</DialogDescription>
      </DialogHeader>
      <div className="flex flex-col gap-8">
        <div className="grid grid-cols-2 gap-x-6 gap-y-4 sm:grid-cols-4">
          {stats.map(stat => <div key={stat.label} className="flex min-w-0 flex-col gap-0.5"><span className="text-xs text-muted-foreground">{stat.label}</span><span className="text-2xl font-semibold tabular-nums">{stat.value}</span></div>)}
        </div>
        {/* An unpriced model's cost is unknown, not zero, so its line shows tokens. */}
        <SpendChart models={[model.model]} colors={new Map([[model.model, color]])} periods={alone.periods} metric={unknown ? 'tokens' : metric} resolution={span.resolution} />
        <div className="grid gap-x-10 gap-y-8 sm:grid-cols-2">
          {!unknown && <ShareBar label="Cost by stage" metric="cost" segments={stageSegments(model.byStage)} />}
          <ShareBar label="Tokens by stage" metric="tokens" segments={stageSegments(model.byStage)} />
        </div>
      </div>
      <DialogFooter className="text-xs text-muted-foreground sm:justify-start">
        {price
          ? `${formatUsd(price.input)} per 1M input${price.output ? `, ${formatUsd(price.output)} per 1M output` : ''} · ${price.source}`
          : 'No cloud model has these weights, so its tokens are counted and left unpriced.'}
      </DialogFooter>
    </DialogContent>
  </Dialog>
}

export function SpendSkeleton() {
  return <div className="flex max-w-5xl flex-col gap-8">
    <section className="grid gap-6 lg:grid-cols-[minmax(0,18rem)_minmax(0,1fr)]">
      <div className="flex flex-col gap-5">
        <span className="text-4xl font-semibold"><Skeleton mask="$12.34" /></span>
        {[0, 1, 2].map(i => <div key={i} className="grid gap-1"><span className="text-sm"><Skeleton mask="qwen/qwen3-embedding-8b  $0.00" /></span><span className="text-xs"><Skeleton mask="OpenRouter · 40.0% of cost · 1.2M tokens" /></span></div>)}
      </div>
      <Skeleton className="h-60 rounded-lg" />
    </section>
    <Skeleton className="h-12 rounded-lg" />
    <Skeleton className="h-40 rounded-lg" />
  </div>
}
