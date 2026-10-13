import { createFileRoute } from '@tanstack/react-router'
import { InvestigatePage, InvestigateSkeleton } from '../../../investigate'

// No loader: an investigation is followed on the job stream until it finishes, then its report is read.
export const Route = createFileRoute('/_workspace/_admin/sources/investigate/$id')({
  pendingComponent: InvestigateSkeleton,
  component: () => <InvestigatePage id={Route.useParams().id} />,
})
