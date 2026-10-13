import { createFileRoute } from '@tanstack/react-router'
import { TryPage, TrySkeleton } from '../../../sources'

// No loader: a try is followed on the job stream until it finishes, then its report is read.
export const Route = createFileRoute('/_workspace/_admin/sources/try/$id')({
  pendingComponent: TrySkeleton,
  component: () => <TryPage id={Route.useParams().id} />,
})
