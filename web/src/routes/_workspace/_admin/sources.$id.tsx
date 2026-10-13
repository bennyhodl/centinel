import { createFileRoute } from '@tanstack/react-router'
import { queries } from '../../../queries'
import { SourcePage, SourceSkeleton } from '../../../sources'

export const Route = createFileRoute('/_workspace/_admin/sources/$id')({
  loader: ({ context: { queryClient }, params }) => queryClient.ensureQueryData(queries.source(params.id)),
  pendingComponent: () => <SourceSkeleton id={Route.useParams().id} />,
  component: () => <SourcePage id={Route.useParams().id} />,
})
