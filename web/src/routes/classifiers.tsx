import { createFileRoute } from '@tanstack/react-router'
import { Classify } from '../classify'

export const Route = createFileRoute('/classifiers')({
  component: Classify,
})
