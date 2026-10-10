import { createFileRoute } from '@tanstack/react-router'
import { queries } from '../queries'
import { Review, REVIEW_QUEUE, ReviewSkeleton } from '../review'

export const Route = createFileRoute('/review')({
  loader: ({ context: { queryClient } }) => {
    void queryClient.prefetchQuery(queries.evaluation())
    return Promise.all([
      queryClient.ensureQueryData(queries.questions()),
      queryClient.ensureQueryData(queries.reviewQueue('', false, REVIEW_QUEUE)),
    ])
  },
  pendingComponent: ReviewSkeleton,
  component: Review,
})
