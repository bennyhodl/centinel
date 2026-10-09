import { useEffect, useState } from 'react'
import { useIsFetching, useIsMutating, useQueryErrorResetBoundary } from '@tanstack/react-query'
import { useRouter, useRouterState, type ErrorComponentProps } from '@tanstack/react-router'
import { RotateCcw } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Skeleton } from '@/components/ui/skeleton'

/**
 * A two-pixel line at the top of the window while the workspace waits on something you
 * asked for: a page loading, a first read, a save. Polling while a run scores is left
 * out, or the line would never rest.
 */
export function ActivityBar() {
  // The shell is drawn at build time with nothing loading; the browser starts mid-load.
  // Drawing the line only once mounted keeps the two from disagreeing.
  const [mounted, setMounted] = useState(false)
  useEffect(() => setMounted(true), [])
  const routing = useRouterState({ select: state => state.status === 'pending' })
  const loading = useIsFetching({ predicate: query => query.state.status === 'pending' }) > 0
  const saving = useIsMutating() > 0
  const active = routing || loading || saving
  if (!mounted) return null
  return <div aria-hidden className="pointer-events-none fixed inset-x-0 top-0 z-50 h-0.5">
    <div key={active ? 'on' : 'off'} className={`h-full bg-flame ${active ? 'animate-activity-creep' : 'w-full opacity-0 transition-opacity duration-(--motion-indicator)'}`} />
  </div>
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
