import { QueryClient } from '@tanstack/react-query'
import { createRouter } from '@tanstack/react-router'
import { RouteError, RoutePending } from './feedback'
import { routeTree } from './routeTree.gen'

// A browser aborts a view transition when another navigation starts or the tab hides,
// and rejects its promises. Nothing here awaits them, so the rejections only reach the
// console as noise.
if (typeof document !== 'undefined' && document.startViewTransition) {
  const start = document.startViewTransition.bind(document)
  document.startViewTransition = ((...args: Parameters<typeof start>) => {
    const transition = start(...args)
    for (const promise of [transition.ready, transition.finished, transition.updateCallbackDone]) promise.catch(() => {})
    return transition
  }) as typeof document.startViewTransition
}

export function getRouter() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { staleTime: 30_000, retry: 1 } } })
  return createRouter({
    routeTree,
    basepath: '/web',
    context: { queryClient },
    // Hovering or focusing a link runs its page's loader, so the data is usually there
    // by the click. The query cache decides what is fresh, not the router.
    defaultPreload: 'intent',
    defaultPreloadStaleTime: 0,
    // A wait under 300 ms shows nothing. Once a skeleton shows, it stays 500 ms, so it
    // never flashes.
    defaultPendingMs: 300,
    defaultPendingMinMs: 500,
    defaultPendingComponent: RoutePending,
    defaultErrorComponent: RouteError,
    // Pages slide the way history moves: forward deeper, back out. Not on first load,
    // and not in a hidden tab.
    defaultViewTransition: {
      types: ({ fromLocation, toLocation }) => {
        if (!fromLocation || document.visibilityState !== 'visible') return false
        const from = (fromLocation.state as { __TSR_index?: number }).__TSR_index ?? 0
        const to = (toLocation.state as { __TSR_index?: number }).__TSR_index ?? 0
        return [to < from ? 'back' : 'forward']
      },
    },
    scrollRestoration: true,
  })
}

declare module '@tanstack/react-router' {
  interface Register { router: ReturnType<typeof getRouter> }
}
