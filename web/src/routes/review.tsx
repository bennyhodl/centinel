import { createFileRoute } from '@tanstack/react-router'
import { Review } from '../review'

export const Route = createFileRoute('/review')({
  component: Review,
})
