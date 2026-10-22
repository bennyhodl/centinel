import { createFileRoute } from '@tanstack/react-router'
import { queries } from '../../queries'
import { PersonPage, PersonSkeleton } from '../../rolodex'

/** One name, whether or not a link carries it: a name nobody links still has the sources that write it. */
export const Route = createFileRoute('/_workspace/rolodex/$name')({
  loader: ({ context: { queryClient }, params }) => queryClient.ensureQueryData(queries.rolodexPerson(params.name)),
  pendingComponent: () => <PersonSkeleton name={Route.useParams().name} />,
  component: () => <PersonPage name={Route.useParams().name} />,
})
