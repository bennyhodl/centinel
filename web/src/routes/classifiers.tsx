import { createFileRoute } from '@tanstack/react-router'
import { Classify, ClassifySkeleton } from '../classify'
import { queries } from '../queries'

export const Route = createFileRoute('/classifiers')({
  loader: ({ context: { queryClient } }) => {
    void queryClient.prefetchQuery(queries.presets())
    return queryClient.ensureQueryData(queries.questions())
  },
  pendingComponent: ClassifySkeleton,
  component: Classify,
})
