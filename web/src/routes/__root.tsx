import type { ReactNode } from 'react'
import { createRootRouteWithContext, HeadContent, Scripts, useRouter } from '@tanstack/react-router'
import { QueryClientProvider, type QueryClient } from '@tanstack/react-query'
import { themeScript } from '../theme'
import { ShellPending } from '../feedback'
import crest from '../assets/crest.jpg'
import styles from '../styles.css?url'

/** The document. Who may see what is decided one level down, by `_entry` and `_workspace`. */
export const Route = createRootRouteWithContext<{ queryClient: QueryClient }>()({
  ssr: false,
  head: () => ({
    meta: [
      { charSet: 'utf-8' },
      { title: 'Centinel' },
      { name: 'viewport', content: 'width=device-width, initial-scale=1' },
      { name: 'color-scheme', content: 'light dark' },
      { name: 'centinel-version', content: __CENTINEL_VERSION__ },
    ],
    links: [{ rel: 'stylesheet', href: styles }, { rel: 'icon', type: 'image/jpeg', href: crest }],
  }),
  shellComponent: RootDocument,
  pendingComponent: ShellPending,
})

/** The query cache wraps everything drawn, the first paint's skeletons included: some read the cache. */
function RootDocument({ children }: { children: ReactNode }) {
  const { queryClient } = useRouter().options.context
  return <html lang="en" suppressHydrationWarning><head><script dangerouslySetInnerHTML={{ __html: themeScript }} /><HeadContent /></head><body>
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider><Scripts />
  </body></html>
}
