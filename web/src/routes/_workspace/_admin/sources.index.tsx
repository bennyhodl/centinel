import { createFileRoute } from '@tanstack/react-router'
import { queries } from '../../../queries'
import { Sources, SourcesSkeleton } from '../../../sources'

export const Route = createFileRoute('/_workspace/_admin/sources/')({
  loader: ({ context: { queryClient } }) => queryClient.ensureQueryData(queries.sources()),
  pendingComponent: SourcesSkeleton,
  component: Sources,
})
