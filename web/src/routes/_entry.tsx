import type { ReactNode } from 'react'
import { createFileRoute, Outlet } from '@tanstack/react-router'
import { PagePending } from '../feedback'

export const Route = createFileRoute('/_entry')({ pendingComponent: () => <Entry><PagePending /></Entry>, component: () => <Entry><Outlet /></Entry> })

/**
 * The pages before the workspace: signing in, and letting an agent in. One column on the
 * page by day; by night a near-black grey, so the form is the only thing lit.
 */
function Entry({ children }: { children: ReactNode }) {
  return <main className="flex min-h-svh flex-col items-center justify-center bg-background p-6 dark:bg-night md:p-10">
    <div className="w-full max-w-sm">{children}</div>
  </main>
}
