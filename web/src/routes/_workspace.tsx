import { useEffect, useState, type ReactNode } from 'react'
import { createFileRoute, Link, Navigate, Outlet, redirect, useLocation } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { BookOpen, CircleDollarSign, Eye, FlaskConical, History, LogOut, Moon, Plug, Search, ShieldCheck, Sparkles, Sun, Users } from 'lucide-react'
import { signOut } from '../auth'
import { queries } from '../queries'
import { JobDrawerProvider, WorkingNow, useJobEvents } from '../jobs'
import { ActivityBar } from '../feedback'
import { quotes } from '../quotes'
import { number } from '../format'
import { useTheme } from '../theme'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Sidebar, SidebarContent, SidebarFooter, SidebarGroup, SidebarGroupLabel, SidebarHeader, SidebarInset, SidebarMenu, SidebarMenuBadge, SidebarMenuButton, SidebarMenuItem, SidebarProvider, SidebarTrigger } from '@/components/ui/sidebar'
import crest from '../assets/crest.jpg'

/**
 * Every page of the workspace sits under here, behind one check: an approved account. Pages
 * read it from the route context. The sign-in page handles everyone else.
 */
export const Route = createFileRoute('/_workspace')({
  beforeLoad: async ({ context: { queryClient } }) => {
    const account = await queryClient.ensureQueryData(queries.account())
    if (account?.access !== 'approved') throw redirect({ to: '/login' })
    return { account }
  },
  component: Workspace,
})

function Workspace() {
  // The account is re-read while the page is open. The moment it is gone or revoked, so is the page.
  const account = useQuery(queries.account()).data
  if (account?.access !== 'approved') return <Navigate to="/login" />
  return <Shell admin={account.role === 'admin'} />
}

function Shell({ admin }: { admin: boolean }) {
  const pathname = useLocation({ select: location => location.pathname })
  const system = useQuery(queries.system())
  const corpus = useQuery(queries.corpus({}, 1, 1))
  const questions = useQuery(queries.questions())
  const review = useQuery({ ...queries.reviewQueue(1), enabled: admin })
  // The rail says when a run is going, from any page.
  const recent = useQuery({
    ...queries.runs(1, 10),
    enabled: admin,
    // Slower while nothing scores, so a run started elsewhere still shows up.
    refetchInterval: query => query.state.data?.runs.some(run => run.status === 'running') ? 2000 : 15000,
  })
  const running = recent.data?.runs.filter(run => run.status === 'running') || []
  useJobEvents(admin)
  const serverVersion = system.data?.version
  const stale = Boolean(serverVersion && serverVersion !== __CENTINEL_VERSION__)
  const at = (path: string) => pathname === path

  return <JobDrawerProvider><SidebarProvider defaultOpen={false} className="bg-ground">
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
          <NavItem active={at('/')} label="Search" count={corpus.data && number(corpus.data.total)}>
            <Link to="/" search={{}}><Search /><span>Search</span></Link>
          </NavItem>
          {admin && <NavItem active={at('/spend')} label="Spend">
            <Link to="/spend" search={{ metric: 'cost', days: 30 }}><CircleDollarSign /><span>Spend</span></Link>
          </NavItem>}
        </NavGroup>
        {admin && <NavGroup label="Classifiers">
          <NavItem active={at('/classifiers')} label="Classify" count={questions.data && number(questions.data.questions.length)}>
            <Link to="/classifiers"><FlaskConical /><span>Classify</span></Link>
          </NavItem>
          <NavItem active={at('/runs')} label="Runs" count={running.length ? <LivePill>{running.length} live</LivePill> : recent.data && number(recent.data.total)}>
            <Link to="/runs" search={{ run: '', page: 1, outcome: '' }}><History /><span>Runs</span></Link>
          </NavItem>
          <NavItem active={at('/review')} label="Review" count={review.data?.in_review_band ? <span className="font-semibold text-flame-ink">{number(review.data.in_review_band)}</span> : undefined}>
            <Link to="/review"><Eye /><span>Review</span></Link>
          </NavItem>
        </NavGroup>}
        <NavGroup label="Agent">
          <NavItem active={at('/connect')} label="Connect" count={<span className="inline-flex items-center gap-1.5"><span className="size-1.5 rounded-full bg-moss" />MCP</span>}>
            <Link to="/connect"><Plug /><span>Connect</span></Link>
          </NavItem>
          <NavItem active={at('/skills')} label="Skills" count="1">
            <Link to="/skills"><Sparkles /><span>Skills</span></Link>
          </NavItem>
        </NavGroup>
      </SidebarContent>
      <SidebarFooter className="gap-3 px-3 pb-3">
        {admin && <WorkingNow />}
        <Quote />
        <SidebarMenu className="-mx-1">
          <NavItem active={pathname.startsWith('/docs')} label="Documentation"><Link to="/docs/$" params={{ _splat: '' }}><BookOpen /><span>Documentation</span></Link></NavItem>
          {admin && <NavItem active={at('/users')} label="User access"><Link to="/users"><Users /><span>User access</span></Link></NavItem>}
          <SidebarMenuItem><SidebarMenuButton onClick={signOut} tooltip="Sign out" className="h-9 text-[14px]"><LogOut /><span>Sign out</span></SidebarMenuButton></SidebarMenuItem>
          <ThemeToggle />
        </SidebarMenu>
      </SidebarFooter>
    </Sidebar>
    <SidebarInset className="min-w-0 bg-background md:my-2.5 md:mr-2.5 md:rounded-2xl md:border md:border-sidebar-border md:shadow-[0_1px_2px_rgba(26,23,18,0.06),0_8px_24px_rgba(26,23,18,0.05)]">
      <div className="flex h-12 items-center px-4 md:hidden"><SidebarTrigger /></div>
      <div className="min-w-0 px-5 py-6 [view-transition-name:page] md:px-10 md:py-8">
        {stale && <Alert className="mb-6"><ShieldCheck /><AlertTitle>Server is v{serverVersion}</AlertTitle><AlertDescription>This page is v{__CENTINEL_VERSION__}. Stop the old `centinel web` and start it again, then reload.</AlertDescription></Alert>}
        <Outlet />
      </div>
    </SidebarInset>
  </SidebarProvider></JobDrawerProvider>
}

function NavGroup({ label, children }: { label: string; children: ReactNode }) {
  return <SidebarGroup className="py-1">
    <SidebarGroupLabel className="text-[11px] font-semibold uppercase tracking-[0.08em] text-muted-foreground">{label}</SidebarGroupLabel>
    <SidebarMenu>{children}</SidebarMenu>
  </SidebarGroup>
}

function NavItem({ active, label, count, children }: { active: boolean; label: string; count?: ReactNode; children: ReactNode }) {
  return <SidebarMenuItem>
    <SidebarMenuButton asChild isActive={active} tooltip={label} className="h-9 text-[14px] data-[active=true]:bg-background data-[active=true]:font-semibold data-[active=true]:shadow-[0_0_0_1px_var(--rule)]">{children}</SidebarMenuButton>
    {count !== undefined && <SidebarMenuBadge className="top-2 font-mono text-xs font-normal text-muted-foreground">{count}</SidebarMenuBadge>}
  </SidebarMenuItem>
}

function ThemeToggle() {
  const [theme, toggle] = useTheme()
  const label = theme === 'dark' ? 'Light mode' : 'Dark mode'
  return <SidebarMenuItem>
    <SidebarMenuButton onClick={toggle} tooltip={label} className="h-9 text-[14px] text-muted-foreground">
      {theme === 'dark' ? <Sun /> : <Moon />}<span>{label}</span>
    </SidebarMenuButton>
  </SidebarMenuItem>
}

function LivePill({ children }: { children: ReactNode }) {
  return <span className="inline-flex items-center gap-1.5 rounded-full bg-flame-soft px-2 py-0.5 font-sans text-[11px] font-semibold text-flame-ink"><span className="size-1.5 rounded-full bg-flame shadow-[0_0_0_3px_var(--flame-halo)]" />{children}</span>
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
    <blockquote className="font-serif text-[14px] leading-[19px] italic text-ink-soft">“{quote.text}”</blockquote>
    <figcaption className="text-[11px] text-muted-foreground">{quote.who}, {quote.where}</figcaption>
  </figure>
}
