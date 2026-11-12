import { createFileRoute } from '@tanstack/react-router'
import { Docs } from '../../docs/Docs'

// The book needs nothing from the server, so it draws itself even while the account is
// read. It is a chunk of its own: no other page pays for the Markdown or its renderer.
export const Route = createFileRoute('/_workspace/docs/$')({ pendingComponent: Docs, component: Docs })
