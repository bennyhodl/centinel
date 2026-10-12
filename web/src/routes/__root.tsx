import type { ReactNode } from 'react'
import { createRootRouteWithContext, HeadContent, Outlet, Scripts } from '@tanstack/react-router'
import { QueryClientProvider, type QueryClient } from '@tanstack/react-query'
import { themeScript } from '../theme'
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
  component: App,
})

function RootDocument({ children }: { children: ReactNode }) {
  return <html lang="en" suppressHydrationWarning><head><script dangerouslySetInnerHTML={{ __html: themeScript }} /><HeadContent /></head><body>{children}<Scripts /></body></html>
}

function App() {
  const { queryClient } = Route.useRouteContext()
  return <QueryClientProvider client={queryClient}><Outlet /></QueryClientProvider>
}
