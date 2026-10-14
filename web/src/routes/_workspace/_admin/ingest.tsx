import { createFileRoute } from '@tanstack/react-router'
import { Ingest } from '../../../ingest'
import { queries } from '../../../queries'

export const Route = createFileRoute('/_workspace/_admin/ingest')({
  loader: ({ context: { queryClient } }) => queryClient.ensureQueryData(queries.sources()),
  component: Ingest,
})
