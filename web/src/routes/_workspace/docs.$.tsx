import { createFileRoute } from '@tanstack/react-router'
import { Docs } from '../../docs/Docs'

export const Route = createFileRoute('/_workspace/docs/$')({
  component: Docs,
})
