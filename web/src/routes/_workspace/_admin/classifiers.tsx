import { createFileRoute } from '@tanstack/react-router'
import { Classify, ClassifySkeleton } from '../../../classify'
import { queries } from '../../../queries'

export const Route = createFileRoute('/_workspace/_admin/classifiers')({
  /** A source to classify, preset by the Corpus page's "Classify this source". */
  validateSearch: (search: Record<string, unknown>): { source?: string } => (search.source ? { source: String(search.source) } : {}),
  loader: ({ context: { queryClient } }) => {
    void queryClient.prefetchQuery(queries.presets())
    return queryClient.ensureQueryData(queries.questions())
  },
  pendingComponent: ClassifySkeleton,
  component: Classify,
})
