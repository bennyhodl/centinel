import { QueryClient } from '@tanstack/react-query'
import { createRouter } from '@tanstack/react-router'
import { routeTree } from './routeTree.gen'

export function getRouter() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { staleTime: 8_000, retry: 1 } } })
  return createRouter({ routeTree, basepath: '/web', context: { queryClient } })
}

declare module '@tanstack/react-router' {
  interface Register { router: ReturnType<typeof getRouter> }
}
