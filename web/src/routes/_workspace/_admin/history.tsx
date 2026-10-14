import { createFileRoute } from '@tanstack/react-router'
import { CollectionHistory } from '../../../history'
import { queries } from '../../../queries'

export const Route = createFileRoute('/_workspace/_admin/history')({
  validateSearch: (search: Record<string, unknown>) => ({
    source: String(search.source || ''), schedule: String(search.schedule || ''), outcome: String(search.outcome || ''),
    page: Number.isSafeInteger(Number(search.page)) && Number(search.page) > 0 ? Number(search.page) : 1,
    run: String(search.run || ''),
  }),
  loaderDeps: ({ search }) => search,
  loader: ({ context: { queryClient }, deps }) => queryClient.ensureQueryData(queries.collectionHistory(deps)),
  component: CollectionHistory,
})
