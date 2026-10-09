import { useEffect, useState, type ReactNode } from 'react'
import { createRootRouteWithContext, HeadContent, Link, Outlet, Scripts, useLocation } from '@tanstack/react-router'
import { QueryClientProvider, useQuery, type QueryClient } from '@tanstack/react-query'
import { Eye, FlaskConical, History, Plug, Search, ShieldCheck, Sparkles } from 'lucide-react'
import { queries } from '../queries'
import { JobDrawer, WorkingNow, useJobEvents } from '../jobs'
import { ActivityBar } from '../feedback'
import { quotes } from '../quotes'
import { number } from '../format'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Sidebar, SidebarContent, SidebarFooter, SidebarGroup, SidebarGroupLabel, SidebarHeader, SidebarInset, SidebarMenu, SidebarMenuBadge, SidebarMenuButton, SidebarMenuItem, SidebarProvider, SidebarTrigger } from '@/components/ui/sidebar'
import crest from '../assets/crest.jpg'
import styles from '../styles.css?url'

export const Route = createRootRouteWithContext<{ queryClient: QueryClient }>()({
  head: () => ({
    meta: [
      { charSet: 'utf-8' },
      { title: 'Centinel' },
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

const searchDefaults = { text: '', address: '', page: 1, source: '', usage: 'all', classifier: '', minScore: '0.5', maxScore: '' }

function Shell() {
  const pathname = useLocation({ select: location => location.pathname })
  const system = useQuery(queries.system())
  const corpus = useQuery(queries.corpus({}, 1, 1))
  const questions = useQuery(queries.questions())
  const review = useQuery(queries.reviewQueue('', false, 1))
  // The rail says when a run is going, from any page.
  const recent = useQuery({
    ...queries.runs(1, 10),
    // Slower while nothing scores, so a run started elsewhere still shows up.
    refetchInterval: query => query.state.data?.runs.some(run => run.status === 'running') ? 2000 : 15000,
  })
  const running = recent.data?.runs.filter(run => run.status === 'running') || []
  useJobEvents()
  const serverVersion = system.data?.version
  const stale = Boolean(serverVersion && serverVersion !== __CENTINEL_VERSION__)
  const [job, setJob] = useState('')
  const at = (path: string) => pathname === `/web${path}` || (path === '/' && pathname === '/web')

  return <SidebarProvider defaultOpen={false} className="bg-ground">
    <ActivityBar />
    <Sidebar variant="floating" collapsible="icon">
      <SidebarHeader className="px-3 pt-4 pb-2">
        <div className="flex items-center gap-3 px-1 group-data-[collapsible=icon]:flex-col group-data-[collapsible=icon]:px-0">
          <img src={crest} alt="" className="size-11 shrink-0 rounded-md border border-foreground object-cover group-data-[collapsible=icon]:size-8" />
          <div className="grid flex-1 group-data-[collapsible=icon]:hidden">
            <span className="font-display text-[22px] leading-6 tracking-[0.08em]">Centinel</span>
            <span className="text-[11px] text-muted-foreground">v{__CENTINEL_VERSION__}</span>
          </div>
          <SidebarTrigger className="size-7 self-start text-muted-foreground group-data-[collapsible=icon]:self-center" />
        </div>
      </SidebarHeader>
      <SidebarContent>
        <NavGroup label="Archive">
          <NavItem active={at('/')} label="Search" icon={<Search />} count={corpus.data && number(corpus.data.total)}>
            <Link to="/" search={searchDefaults}><Search /><span>Search</span></Link>
          </NavItem>
        </NavGroup>
        <NavGroup label="Classifiers">
          <NavItem active={at('/classifiers')} label="Classify" count={questions.data && number(questions.data.questions.length)}>
            <Link to="/classifiers"><FlaskConical /><span>Classify</span></Link>
          </NavItem>
          <NavItem active={at('/runs')} label="Runs" count={running.length ? <LivePill>{running.length} live</LivePill> : recent.data && number(recent.data.total)}>
            <Link to="/runs" search={{ run: '', page: 1, outcome: '' }}><History /><span>Runs</span></Link>
          </NavItem>
          <NavItem active={at('/review')} label="Review" count={review.data?.in_review_band ? <span className="font-semibold text-flame-ink">{number(review.data.in_review_band)}</span> : undefined}>
            <Link to="/review"><Eye /><span>Review</span></Link>
          </NavItem>
        </NavGroup>
        <NavGroup label="Agent">
          <NavItem active={at('/connect')} label="Connect" count={<span className="inline-flex items-center gap-1.5"><span className="size-1.5 rounded-full bg-[#4F7A4A]" />MCP</span>}>
            <Link to="/connect"><Plug /><span>Connect</span></Link>
          </NavItem>
          <NavItem active={at('/skills')} label="Skills" count="1">
            <Link to="/skills"><Sparkles /><span>Skills</span></Link>
          </NavItem>
        </NavGroup>
      </SidebarContent>
      <SidebarFooter className="gap-3 px-3 pb-3">
        <WorkingNow onOpen={setJob} />
        <Quote />
      </SidebarFooter>
    </Sidebar>
    <SidebarInset className="min-w-0 bg-background md:my-2.5 md:mr-2.5 md:rounded-2xl md:border md:border-sidebar-border md:shadow-[0_1px_2px_rgba(26,23,18,0.06),0_8px_24px_rgba(26,23,18,0.05)]">
      <div className="flex h-12 items-center px-4 md:hidden"><SidebarTrigger /></div>
      <div className="min-w-0 px-5 py-6 [view-transition-name:page] md:px-10 md:py-8">
        {stale && <Alert className="mb-6"><ShieldCheck /><AlertTitle>Server is v{serverVersion}</AlertTitle><AlertDescription>This page is v{__CENTINEL_VERSION__}. Stop the old `centinel web` and start it again, then reload.</AlertDescription></Alert>}
        <Outlet />
      </div>
    </SidebarInset>
    {job && <JobDrawer id={job} onClose={() => setJob('')} />}
  </SidebarProvider>
}

function NavGroup({ label, children }: { label: string; children: ReactNode }) {
  return <SidebarGroup className="py-1">
    <SidebarGroupLabel className="text-[11px] font-semibold uppercase tracking-[0.08em] text-muted-foreground">{label}</SidebarGroupLabel>
    <SidebarMenu>{children}</SidebarMenu>
  </SidebarGroup>
}

function NavItem({ active, label, count, children }: { active: boolean; label: string; icon?: ReactNode; count?: ReactNode; children: ReactNode }) {
  return <SidebarMenuItem>
    <SidebarMenuButton asChild isActive={active} tooltip={label} className="h-9 text-[14px] data-[active=true]:bg-background data-[active=true]:font-semibold data-[active=true]:shadow-[0_0_0_1px_var(--rule)]">{children}</SidebarMenuButton>
    {count !== undefined && <SidebarMenuBadge className="top-2 font-mono text-xs font-normal text-muted-foreground">{count}</SidebarMenuBadge>}
  </SidebarMenuItem>
}

function LivePill({ children }: { children: ReactNode }) {
  return <span className="inline-flex items-center gap-1.5 rounded-full bg-flame-soft px-2 py-0.5 font-sans text-[11px] font-semibold text-flame-ink"><span className="size-1.5 rounded-full bg-flame shadow-[0_0_0_3px_#F6D9B4]" />{children}</span>
}

/**
 * One of the Founders, a different one each time the page loads. Chosen after mount: the
 * shell is rendered once at build time, and a pick made there would disagree with the
 * browser's.
 */
function Quote() {
  const [quote, setQuote] = useState<(typeof quotes)[number]>()
  useEffect(() => setQuote(quotes[Math.floor(Math.random() * quotes.length)]), [])
  if (!quote) return null
  return <figure className="grid gap-1.5 border-t px-1 pt-3 group-data-[collapsible=icon]:hidden">
    <blockquote className="font-serif text-[14px] leading-[19px] italic text-[#3A352D]">“{quote.text}”</blockquote>
    <figcaption className="text-[11px] text-muted-foreground">{quote.who}, {quote.where}</figcaption>
  </figure>
}
