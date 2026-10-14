import { createFileRoute } from '@tanstack/react-router'
import { queries } from '../../../queries'
import { SystemHealthPage } from '../../../system-health'

export const Route = createFileRoute('/_workspace/_admin/system')({
  loader: ({ context: { queryClient } }) => queryClient.ensureQueryData(queries.systemHealth()),
  component: SystemHealthPage,
})
