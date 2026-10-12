import { createFileRoute, redirect } from '@tanstack/react-router'

/** Classifying, reviewing, spend and access are the administrator's. Anyone else lands on search. */
export const Route = createFileRoute('/_workspace/_admin')({
  beforeLoad: ({ context: { account } }) => {
    if (account.role !== 'admin') throw redirect({ to: '/', search: {} })
  },
})
