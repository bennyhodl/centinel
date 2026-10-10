import { useLayoutEffect, useMemo, useRef, useState } from 'react'
import { formatPeriod, formatTokens, formatUsd, modelLabel, type Metric, type PeriodTotals, type Window } from './spend-logic'

/**
 * Spend over the window, one line per model, each measured from zero rather than stacked.
 * T3 Code's usage chart, ported: hand-drawn SVG, monotone curves that cannot overshoot a
 * spike, a 1/2/5 scale rounded up past the peak, and a readout that follows the pointer.
 */

const VIEW_WIDTH = 960
const VIEW_HEIGHT = 260
const TICK_COUNT = 4
const PLOT_TOP = 8

type Point = { x: number; y: number }

/** Shape-preserving cubic tangents (Fritsch–Carlson), so a quiet day stays at zero. */
function monotoneTangents(points: Point[]) {
  const n = points.length
  if (n < 2) return [0]
  const slopes = points.slice(1).map((p, i) => {
    const dx = p.x - points[i].x
    return dx === 0 ? 0 : (p.y - points[i].y) / dx
  })
  const tangents = Array.from({ length: n }, (_, i) =>
    i === 0 ? slopes[0] : i === n - 1 ? slopes[n - 2] : slopes[i - 1] * slopes[i] <= 0 ? 0 : (slopes[i - 1] + slopes[i]) / 2)
  slopes.forEach((slope, i) => {
    if (slope === 0) { tangents[i] = 0; tangents[i + 1] = 0; return }
    const a = tangents[i] / slope
    const b = tangents[i + 1] / slope
    const magnitude = a * a + b * b
    if (magnitude > 9) {
      const scale = 3 / Math.sqrt(magnitude)
      tangents[i] = scale * a * slope
      tangents[i + 1] = scale * b * slope
    }
  })
  return tangents
}

function curvePath(points: Point[]) {
  if (points.length < 2) return ''
  const t = monotoneTangents(points)
  let path = `M${points[0].x.toFixed(2)},${points[0].y.toFixed(2)}`
  for (let i = 0; i < points.length - 1; i += 1) {
    const from = points[i]
    const to = points[i + 1]
    const dx = to.x - from.x
    path += ` C${(from.x + dx / 3).toFixed(2)},${(from.y + t[i] * dx / 3).toFixed(2)} ${(to.x - dx / 3).toFixed(2)},${(to.y - t[i + 1] * dx / 3).toFixed(2)} ${to.x.toFixed(2)},${to.y.toFixed(2)}`
  }
  return path
}

/**
 * A readable 1/2/5 × 10ⁿ maximum at or above the peak. Rounding up is the point: the
 * last step below would draw the tallest day past the top of the plot.
 */
export function niceScale(peak: number, count = TICK_COUNT) {
  if (peak <= 0) return { max: 0, ticks: [0] }
  const raw = peak / count
  const magnitude = 10 ** Math.floor(Math.log10(raw))
  const normalized = raw / magnitude
  const step = (normalized > 5 ? 10 : normalized > 2 ? 5 : normalized > 1 ? 2 : 1) * magnitude
  const max = Math.ceil(peak / step) * step
  const ticks: number[] = []
  for (let value = 0; value <= max + step * 1e-6; value += step) ticks.push(value)
  return { max, ticks }
}

// Room above the top gridline, so a series at the peak is not clipped.
const toY = (value: number, max: number) => max === 0 ? VIEW_HEIGHT : VIEW_HEIGHT - value / max * (VIEW_HEIGHT - PLOT_TOP)

export function SpendChart({ models, colors, periods, metric, resolution }: {
  models: string[]
  colors: Map<string, string>
  periods: PeriodTotals[]
  metric: Metric
  resolution: Window['resolution']
}) {
  const valueOf = (period: PeriodTotals | undefined, model: string) => {
    const figures = period?.byModel.get(model)
    return figures ? metric === 'cost' ? figures.costUsd : figures.tokens : 0
  }
  const { scale, paths, stepX } = useMemo(() => {
    // Not the sum: each series measures from zero, so a combined peak would leave the
    // plot half empty.
    const peak = Math.max(0, ...periods.flatMap(period => models.map(model => valueOf(period, model))))
    const scale = niceScale(peak)
    const stepX = periods.length < 2 ? 0 : VIEW_WIDTH / (periods.length - 1)
    const paths = models.map(model => {
      const line = curvePath(periods.map((period, i) => ({ x: i * stepX, y: toY(valueOf(period, model), scale.max) })))
      return { model, line, area: line && `${line} L${VIEW_WIDTH},${VIEW_HEIGHT} L0,${VIEW_HEIGHT} Z`, total: periods.reduce((sum, period) => sum + valueOf(period, model), 0) }
    })
    // Heavier series first, so the lighter one is not buried.
    return { scale, stepX, paths: [...paths].sort((a, b) => b.total - a.total) }
  }, [periods, models, metric])

  const [hover, setHover] = useState<number | null>(null)
  const [pointer, setPointer] = useState<Point>({ x: 0, y: 0 })
  const plotRef = useRef<HTMLDivElement>(null)
  const tipRef = useRef<HTMLDivElement>(null)
  const [tip, setTip] = useState<Point>({ x: 0, y: 0 })
  const format = metric === 'tokens' ? formatTokens : formatUsd

  // Beside the pointer, flipped to the other side when it would leave the plot.
  useLayoutEffect(() => {
    const plot = plotRef.current
    const box = tipRef.current
    if (hover === null || !plot || !box) return
    const gap = 12
    const left = pointer.x + gap + box.offsetWidth <= plot.clientWidth ? pointer.x + gap : pointer.x - gap - box.offsetWidth
    const top = pointer.y + gap + box.offsetHeight <= plot.clientHeight ? pointer.y + gap : pointer.y - gap - box.offsetHeight
    setTip({ x: Math.min(Math.max(0, left), Math.max(0, plot.clientWidth - box.offsetWidth)), y: Math.min(Math.max(0, top), Math.max(0, plot.clientHeight - box.offsetHeight)) })
  }, [hover, pointer])

  const move = (event: React.MouseEvent<HTMLDivElement>) => {
    const bounds = plotRef.current?.getBoundingClientRect()
    if (!bounds?.width || !periods.length) return
    const x = Math.min(bounds.width, Math.max(0, event.clientX - bounds.left))
    const y = Math.min(bounds.height, Math.max(0, event.clientY - bounds.top))
    setPointer({ x, y })
    setHover(Math.round(x / bounds.width * (periods.length - 1)))
  }

  const hovered = hover === null ? undefined : periods[hover]
  const label = (i: number) => periods[i] ? formatPeriod(periods[i].period, resolution) : ''
  return <div className="flex flex-col gap-1">
    <div className="flex gap-2">
      {/* Axis labels sit outside the plot so they stay on their gridlines. */}
      <div className="relative h-56 w-14 shrink-0">
        {scale.ticks.map(tick => <span key={tick} className="absolute right-0 -translate-y-1/2 text-[10px] text-muted-foreground tabular-nums" style={{ top: `${toY(tick, scale.max) / VIEW_HEIGHT * 100}%` }}>{tick === 0 ? '0' : format(tick)}</span>)}
      </div>
      <div ref={plotRef} className="relative h-56 flex-1" onMouseMove={move} onMouseLeave={() => setHover(null)}>
        <svg className="h-full w-full" viewBox={`0 0 ${VIEW_WIDTH} ${VIEW_HEIGHT}`} preserveAspectRatio="none" role="img" aria-label={`${resolution === 'hour' ? 'Hourly' : 'Daily'} ${metric === 'tokens' ? 'tokens' : 'cost'} by model`}>
          {scale.ticks.map(tick => <line key={tick} x1={0} x2={VIEW_WIDTH} y1={toY(tick, scale.max)} y2={toY(tick, scale.max)} stroke="currentColor" strokeWidth={1} className="text-border" vectorEffect="non-scaling-stroke" />)}
          {/* Fills first, then every stroke, so no series covers another's line. */}
          {paths.map(({ model, area }) => <path key={model} d={area} fill={colors.get(model)} fillOpacity={0.12} />)}
          {paths.map(({ model, line }) => <path key={model} d={line} fill="none" stroke={colors.get(model)} strokeWidth={2} vectorEffect="non-scaling-stroke" />)}
          {hover !== null && <line x1={hover * stepX} x2={hover * stepX} y1={PLOT_TOP} y2={VIEW_HEIGHT} stroke="currentColor" strokeWidth={1} className="text-muted-foreground" vectorEffect="non-scaling-stroke" />}
        </svg>
        {hovered && <div ref={tipRef} className="pointer-events-none absolute z-10 min-w-40 max-w-full rounded-xl border border-border/50 bg-popover/85 px-2.5 py-2 text-xs shadow-lg backdrop-blur-md" style={{ left: tip.x, top: tip.y }}>
          <div className="mb-1 text-muted-foreground">{formatPeriod(hovered.period, resolution)}</div>
          {models.map(model => <div key={model} className="flex items-center justify-between gap-3">
            <span className="flex min-w-0 items-center gap-1.5 text-muted-foreground"><span className="size-2 shrink-0 rounded-full" style={{ backgroundColor: colors.get(model) }} /><span className="truncate">{modelLabel(model)}</span></span>
            <span className="tabular-nums text-foreground">{format(valueOf(hovered, model))}</span>
          </div>)}
          <div className="mt-1 flex items-center justify-between gap-3 border-t border-border pt-1"><span className="text-muted-foreground">Total</span><span className="tabular-nums text-foreground">{format(metric === 'cost' ? hovered.costUsd : hovered.tokens)}</span></div>
        </div>}
      </div>
    </div>
    <div className="flex justify-between pl-16 text-[10px] uppercase text-muted-foreground">
      <span>{label(0)}</span><span>{label(Math.floor(periods.length / 2))}</span><span>{label(periods.length - 1)}</span>
    </div>
  </div>
}
