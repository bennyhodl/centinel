import { useEffect, useState, useSyncExternalStore } from 'react'
import { useIsFetching, useIsMutating, useQueryErrorResetBoundary } from '@tanstack/react-query'
import { useRouter, useRouterState, type ErrorComponentProps } from '@tanstack/react-router'
import { RotateCcw } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Skeleton } from '@/components/ui/skeleton'

/**
 * What the workspace is waiting on that you asked for: a page loading, a first read, a
 * save or a run starting. Polling while a run scores is left out, or it would never rest.
 * Null when nothing is.
 */
export function useWaiting(): string | null {
  const routing = useRouterState({ select: state => state.status === 'pending' })
  const reading = useIsFetching({ predicate: query => query.state.status === 'pending' }) > 0
  const working = useIsMutating() > 0
  return working ? 'Working' : routing ? 'Opening the page' : reading ? 'Reading the archive' : null
}

/** Whole seconds since `active` turned on, ticking while it stays on. Zero when off. */
export function useElapsed(active: boolean) {
  const [seconds, setSeconds] = useState(0)
  useEffect(() => {
    setSeconds(0)
    if (!active) return
    const start = Date.now()
    const timer = setInterval(() => setSeconds(Math.floor((Date.now() - start) / 1000)), 250)
    return () => clearInterval(timer)
  }, [active])
  return active ? seconds : 0
}

// How many waits are on screen where they happen. While any is, the candle at the foot
// of the window stays down rather than say the same thing twice.
let inPlace = 0
const watchers = new Set<() => void>()
const announce = () => watchers.forEach(watcher => watcher())

/** For a component that shows a wait where it happens: while mounted, it is the one that says so. */
export function useShownInPlace() {
  useEffect(() => {
    inPlace++
    announce()
    return () => { inPlace--; announce() }
  }, [])
}

const useAnyInPlace = () => useSyncExternalStore(watcher => { watchers.add(watcher); return () => { watchers.delete(watcher) } }, () => inPlace > 0, () => false)

/**
 * Feedback for every wait, whatever the page. A lit line across the top from the first
 * moment, and once a wait passes two seconds, unless the page shows it itself, a candle at the foot of the window that
 * says what it is and how long it has taken, so a thirty-second search never looks hung.
 */
export function ActivityBar() {
  // The shell is drawn at build time with nothing loading; the browser starts mid-load.
  // Drawing only once mounted keeps the two from disagreeing.
  const [mounted, setMounted] = useState(false)
  useEffect(() => setMounted(true), [])
  const waiting = useWaiting()
  const seconds = useElapsed(waiting !== null)
  const shown = useAnyInPlace()
  if (!mounted) return null
  return <>
    <div aria-hidden className="pointer-events-none fixed inset-x-0 top-0 z-50 h-0.5">
      <div key={waiting ? 'on' : 'off'} className={`h-full bg-flame ${waiting ? 'bg-[linear-gradient(90deg,transparent,#FFE3B8,transparent)] bg-[length:35%_100%] bg-no-repeat shadow-[0_0_10px_rgba(200,118,30,0.7)] [animation:var(--animate-activity-creep),var(--animate-activity-glint)]' : 'w-full opacity-0 transition-opacity duration-(--motion-indicator)'}`} />
    </div>
    {waiting && !shown && seconds >= 2 && <div role="status" className="fixed bottom-6 left-1/2 z-50 flex -translate-x-1/2 animate-waiting-rise items-center gap-2.5 rounded-full bg-foreground py-2 pr-4 pl-3 text-[13px] text-parchment shadow-[0_10px_30px_rgba(26,23,18,0.25)]">
      <Candle />
      <span>{waiting}</span>
      <span className="font-mono text-xs tabular-nums text-parchment/60">{seconds}s</span>
    </div>}
  </>
}

/** A small flame that wavers while something is under way. */
export function Candle({ className = '' }: { className?: string }) {
  return <span aria-hidden className={`relative inline-block size-3 shrink-0 ${className}`}>
    <span className="absolute inset-0 animate-flicker rounded-[50%_50%_50%_50%/60%_60%_40%_40%] bg-[radial-gradient(circle_at_50%_70%,#FFE3B8,#F0A04B_45%,#C8761E_75%)] shadow-[0_0_10px_rgba(240,160,75,0.8)]" />
  </span>
}

/** What a page shows while its loader runs, when the page has no skeleton of its own. */
export function RoutePending() {
  return <div aria-busy className="grid gap-7">
    <Skeleton className="h-11 w-56" />
    <Skeleton className="h-4 w-96 max-w-full" />
    <div className="grid gap-3"><Skeleton className="h-24" /><Skeleton className="h-24" /><Skeleton className="h-24" /></div>
  </div>
}

/** A page whose read failed: what went wrong, and one click to read it again. */
export function RouteError({ error }: ErrorComponentProps) {
  const router = useRouter()
  const { reset } = useQueryErrorResetBoundary()
  useEffect(() => { reset() }, [reset])
  return <div className="grid max-w-xl gap-4 py-10">
    <h1 className="font-serif text-[40px] leading-[44px]">This page could not load</h1>
    <p className="text-sm leading-relaxed text-muted-foreground">{error instanceof Error ? error.message : String(error)}</p>
    <Button className="w-fit" onClick={() => { reset(); void router.invalidate() }}><RotateCcw />Try again</Button>
  </div>
}
