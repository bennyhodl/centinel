import { createFileRoute } from '@tanstack/react-router'
import { queries } from '../../../queries'
import { Schedules } from '../../../schedules'

export const Route = createFileRoute('/_workspace/_admin/schedules')({
  loader: ({ context: { queryClient } }) => Promise.all([queryClient.ensureQueryData(queries.schedules()), queryClient.ensureQueryData(queries.sources())]),
  component: Schedules,
})
