import { createFileRoute } from '@tanstack/react-router'
import { Docs } from '../../docs/Docs'

// The book ships with the page, so it draws itself even while the account is read.
export const Route = createFileRoute('/_workspace/docs/$')({
  pendingComponent: Book,
  component: Book,
})

function Book() {
  return <Docs />
}
