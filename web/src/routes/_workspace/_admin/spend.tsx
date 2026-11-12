import { createFileRoute, stripSearchParams } from '@tanstack/react-router'
import { queries } from '../../../queries'
import { Spend, SpendSkeleton } from '../../../spend'
import { spendSearch, windowFor } from '../../../spend-logic'

export const Route = createFileRoute('/_workspace/_admin/spend')({
  validateSearch: spendSearch,
  search: { middlewares: [stripSearchParams(spendSearch({}))] },
  loaderDeps: ({ search }) => ({ days: search.days }),
  loader: ({ context: { queryClient }, deps }) => {
    void queryClient.prefetchQuery(queries.prices())
    return queryClient.ensureQueryData(queries.spend(windowFor(deps.days).since))
  },
  pendingComponent: SpendSkeleton,
  component: Spend,
})
