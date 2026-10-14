import { createFileRoute } from '@tanstack/react-router'
import { VectorMaintenance } from '../../../vector-maintenance'

export const Route = createFileRoute('/_workspace/_admin/maintenance')({
  validateSearch: (search: Record<string, unknown>): { job?: string } => search.job ? { job: String(search.job) } : {},
  component: Maintenance,
})

function Maintenance() {
  const { job = '' } = Route.useSearch()
  const navigate = Route.useNavigate()
  return <VectorMaintenance key={job} reportId={job} onReport={id => { void navigate({ search: { job: id } }) }} />
}
