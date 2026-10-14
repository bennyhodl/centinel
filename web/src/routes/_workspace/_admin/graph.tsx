import { createFileRoute, type SearchSchemaInput } from '@tanstack/react-router'
import { GraphPage, GraphSkeleton } from '../../../graph'
import { queries } from '../../../queries'

/** The filter lives in the address, so a view can be shared and the back button undoes a change. Every param has a default, so a link may name none. */
const defaults = { min: 5, refused: false }

export const Route = createFileRoute('/_workspace/_admin/graph')({
  validateSearch: (search: Record<string, unknown> & SearchSchemaInput): { min: number; refused: boolean } => ({
    min: Number(search.min) > 0 ? Number(search.min) : defaults.min,
    refused: search.refused === true || search.refused === 'true',
  }),
  loaderDeps: ({ search }) => search,
  loader: ({ context: { queryClient }, deps }) => queryClient.ensureQueryData(queries.graph({ min_pages: deps.min, hide_refused: deps.refused })),
  pendingComponent: GraphSkeleton,
  component: GraphPage,
})
