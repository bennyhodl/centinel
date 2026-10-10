import { createFileRoute } from '@tanstack/react-router'
import { queries } from '../queries'
import { Spend, SpendSkeleton, spendSearch } from '../spend'
import { windowFor } from '../spend-logic'

export const Route = createFileRoute('/spend')({
  validateSearch: spendSearch,
  loaderDeps: ({ search }) => ({ days: search.days }),
  loader: ({ context: { queryClient }, deps }) => {
    void queryClient.prefetchQuery(queries.prices())
    return queryClient.ensureQueryData(queries.spend(windowFor(deps.days).since))
  },
  pendingComponent: SpendSkeleton,
  component: Spend,
})
