import { createFileRoute } from '@tanstack/react-router'
import { queries } from '../queries'
import { Runs, RunsSkeleton, startView } from '../runs'

export const Route = createFileRoute('/runs')({
  validateSearch: (search: Record<string, unknown>) => ({ run: String(search.run || ''), page: Math.max(1, Number(search.page || 1)), outcome: String(search.outcome || '') }),
  loaderDeps: ({ search }) => search,
  // One run's detail when a run is open, the ledger page otherwise.
  loader: ({ context: { queryClient }, deps }) => deps.run
    ? queryClient.ensureQueryData(queries.run(deps.run, startView(deps.outcome)))
    : queryClient.ensureQueryData(queries.runs(deps.page)),
  pendingComponent: RunsSkeleton,
  component: Runs,
})
