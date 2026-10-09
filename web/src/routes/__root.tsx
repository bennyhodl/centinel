import type { ReactNode } from 'react'
import { createRootRouteWithContext, HeadContent, Link, Outlet, Scripts, useLocation } from '@tanstack/react-router'
import { QueryClientProvider, useQuery, type QueryClient } from '@tanstack/react-query'
import { Archive, Eye, FlaskConical, History, ShieldCheck } from 'lucide-react'
import { api } from '../api'
import { Pulse } from '../ui'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Sidebar, SidebarContent, SidebarFooter, SidebarHeader, SidebarInset, SidebarMenu, SidebarMenuButton, SidebarMenuItem, SidebarProvider, SidebarTrigger } from '@/components/ui/sidebar'
import styles from '../styles.css?url'

export const Route = createRootRouteWithContext<{ queryClient: QueryClient }>()({
  head: () => ({
    meta: [
      { charSet: 'utf-8' },
      { title: 'Centinel corpus workspace' },
      { name: 'viewport', content: 'width=device-width, initial-scale=1' },
      { name: 'color-scheme', content: 'light' },
      { name: 'centinel-version', content: __CENTINEL_VERSION__ },
    ],
    links: [{ rel: 'stylesheet', href: styles }],
  }),
  shellComponent: RootDocument,
  component: App,
})

function RootDocument({ children }: { children: ReactNode }) {
  return <html lang="en"><head><HeadContent /></head><body>{children}<Scripts /></body></html>
}

function App() {
  const { queryClient } = Route.useRouteContext()
  return <QueryClientProvider client={queryClient}><Shell /></QueryClientProvider>
}

function Shell() {
  const pathname = useLocation({ select: location => location.pathname })
  const system = useQuery({ queryKey: ['system'], queryFn: api.system, staleTime: 60_000 })
  const recent = useQuery({
    queryKey: ['runs', 'rail'],
    queryFn: () => api.runs(1, 10),
    refetchInterval: query => query.state.data?.runs.some(run => run.status === 'running') ? 2000 : 15000,
  })
  const running = recent.data?.runs.filter(run => run.status === 'running') || []
  const serverVersion = system.data?.version
  const stale = Boolean(serverVersion && serverVersion !== __CENTINEL_VERSION__)
  return <SidebarProvider>
    <Sidebar collapsible="icon">
      <SidebarHeader className="p-4 group-data-[collapsible=icon]:hidden"><b>Centinel</b><small className="text-muted-foreground">Corpus workspace · v{__CENTINEL_VERSION__}</small></SidebarHeader>
      <SidebarContent><SidebarMenu className="px-2">
        <SidebarMenuItem><SidebarMenuButton asChild isActive={pathname === '/web' || pathname === '/web/'} tooltip="Corpus"><Link to="/" search={{ text: '', address: '', page: 1, source: '', usage: 'all', classifier: '', minScore: '0.5', maxScore: '' }}><Archive /><span>Corpus</span></Link></SidebarMenuButton></SidebarMenuItem>
        <SidebarMenuItem><SidebarMenuButton asChild isActive={pathname === '/web/classifiers'} tooltip="Classify"><Link to="/classifiers"><FlaskConical /><span>Classify</span></Link></SidebarMenuButton></SidebarMenuItem>
        <SidebarMenuItem><SidebarMenuButton asChild isActive={pathname === '/web/runs'} tooltip="Runs"><Link to="/runs" search={{ run: running[0]?.id || '', page: 1, outcome: '' }}><History /><span>Runs</span>{running.length > 0 && <span className="ml-auto inline-flex items-center gap-1 text-xs"><Pulse />{running.length} running</span>}</Link></SidebarMenuButton></SidebarMenuItem>
        <SidebarMenuItem><SidebarMenuButton asChild isActive={pathname === '/web/review'} tooltip="Review"><Link to="/review"><Eye /><span>Review</span></Link></SidebarMenuButton></SidebarMenuItem>
      </SidebarMenu></SidebarContent>
      <SidebarFooter className="p-4 group-data-[collapsible=icon]:hidden"><div className="flex gap-2 text-xs"><ShieldCheck className="size-4 shrink-0" /><div><b>Archive stays intact</b><p className="mt-1 text-muted-foreground">Classification changes corpus usage. Collected bytes and the log do not change.</p></div></div></SidebarFooter>
    </Sidebar>
    <SidebarInset className="min-w-0"><header className="flex h-12 items-center border-b px-4"><SidebarTrigger /></header><div className="min-w-0 p-4 md:p-8">
      {stale && <Alert className="mb-4"><ShieldCheck /><AlertTitle>Server is v{serverVersion}</AlertTitle><AlertDescription>This page is v{__CENTINEL_VERSION__}. Stop the old `centinel web` and start it again, then reload.</AlertDescription></Alert>}
      <Outlet />
    </div></SidebarInset>
  </SidebarProvider>
}
