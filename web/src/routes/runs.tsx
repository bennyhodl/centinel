import { createFileRoute } from '@tanstack/react-router'
import { Runs } from '../runs'

export const Route = createFileRoute('/runs')({
  validateSearch: (search: Record<string, unknown>) => ({ run: String(search.run || ''), page: Math.max(1, Number(search.page || 1)), outcome: String(search.outcome || '') }),
  component: Runs,
})
