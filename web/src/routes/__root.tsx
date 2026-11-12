import type { ReactNode } from 'react'
import { createRootRouteWithContext, HeadContent, Scripts, useRouter } from '@tanstack/react-router'
import { QueryClientProvider, type QueryClient } from '@tanstack/react-query'
import { themeScript } from '../theme'
import { ShellPending } from '../feedback'
import crest from '../assets/crest.jpg'
import styles from '../styles.css?url'
import inter from '@fontsource-variable/inter/files/inter-latin-wght-normal.woff2?url'
import fell from '@fontsource/im-fell-english/files/im-fell-english-latin-400-normal.woff2?url'
import fellCaps from '@fontsource/im-fell-english-sc/files/im-fell-english-sc-latin-400-normal.woff2?url'

/** The faces every page opens with: body text, headings, the wordmark. Fetched beside the stylesheet, not after it, so the first paint has its own type. */
const fonts = [inter, fell, fellCaps].map(href => ({ rel: 'preload', as: 'font', type: 'font/woff2', href, crossOrigin: 'anonymous' as const }))

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
    links: [
      { rel: 'stylesheet', href: styles }, ...fonts, { rel: 'icon', type: 'image/jpeg', href: crest },
      // Every page starts by asking who is signed in. The browser asks while the scripts
      // load, and `api.account` picks that answer up instead of asking again.
      { rel: 'preload', as: 'fetch', href: '/api/account', crossOrigin: 'anonymous' },
    ],
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
