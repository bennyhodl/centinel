import { createFileRoute, type SearchSchemaInput } from '@tanstack/react-router'
import type { RolodexFilter } from '../../api'
import { queries } from '../../queries'
import { RolodexPage, RolodexSkeleton } from '../../rolodex'

/** The search and the source live in the address, so a list can be shared and the back button undoes a change. */
export const Route = createFileRoute('/_workspace/rolodex/')({
  validateSearch: (search: Record<string, unknown> & SearchSchemaInput): RolodexFilter => ({
    name: typeof search.name === 'string' ? search.name : '',
    source: typeof search.source === 'string' ? search.source : '',
  }),
  loaderDeps: ({ search }) => search,
  loader: ({ context: { queryClient }, deps }) => queryClient.ensureQueryData(queries.rolodex(deps)),
  pendingComponent: RolodexSkeleton,
  component: RolodexPage,
})
