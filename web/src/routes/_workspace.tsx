import { useEffect, useState, type ReactNode } from 'react'
import { createFileRoute, Link, Navigate, Outlet, redirect, useLocation } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { BookOpen, ChevronsUpDown, CircleDollarSign, Eye, FlaskConical, Globe, History, LogOut, Moon, Plug, Search, ShieldCheck, Sparkles, Sun, Users, Waypoints } from 'lucide-react'
import { signOut } from '../auth'
import { queries } from '../queries'
import { JobDrawerProvider, WorkingNow, useJobEvents } from '../jobs'
import { ActivityBar, PagePending } from '../feedback'
import { quotes } from '../quotes'
import { number } from '../format'
import { useTheme } from '../theme'
import { handle, Portrait } from '../ui'
import type { Account } from '../api'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { DropdownMenu, DropdownMenuContent, DropdownMenuGroup, DropdownMenuItem, DropdownMenuLabel, DropdownMenuSeparator, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Skeleton } from '@/components/ui/skeleton'
import { Sidebar, SidebarContent, SidebarFooter, SidebarGroup, SidebarGroupLabel, SidebarHeader, SidebarInset, SidebarMenu, SidebarMenuBadge, SidebarMenuButton, SidebarMenuItem, SidebarProvider, SidebarTrigger, useSidebar } from '@/components/ui/sidebar'
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
  pendingComponent: WorkspacePending,
  component: Workspace,
})

function Workspace() {
  // The account is re-read while the page is open. The moment it is gone or revoked, so is the page.
  const account = useQuery(queries.account()).data
  if (account?.access !== 'approved') return <Navigate to="/login" />
  return <Shell account={account} />
}

function Shell({ account }: { account: Account }) {
  const admin = account.role === 'admin'
  const pathname = useLocation({ select: location => location.pathname })
  const system = useQuery(queries.system())
  const corpus = useQuery(queries.corpus({}, 1, 1))
  const questions = useQuery(queries.questions())
  const sources = useQuery({ ...queries.sources(), enabled: admin })
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

  return <JobDrawerProvider>
    <ActivityBar />
    <Frame nav={<>
      <NavGroup label="Archive">
        <NavItem active={at('/')} label="Search" count={corpus.data && number(corpus.data.total)}>
          <Link to="/" search={{}}><Search /><span>Search</span></Link>
        </NavItem>
        {admin && <NavItem active={pathname.startsWith('/sources')} label="Sources" count={sources.data && number(sources.data.sources.length)}>
          <Link to="/sources"><Globe /><span>Sources</span></Link>
        </NavItem>}
        {admin && <NavItem active={at('/graph')} label="Graph">
          <Link to="/graph" search={{ min: 5, refused: false }}><Waypoints /><span>Graph</span></Link>
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
    </>} footer={<>
      {admin && <WorkingNow />}
      <Quote />
      <SidebarMenu className="-mx-1"><AccountMenu account={account} /></SidebarMenu>
    </>}>
      {stale && <Alert className="mb-6"><ShieldCheck /><AlertTitle>Server is v{serverVersion}</AlertTitle><AlertDescription>This page is v{__CENTINEL_VERSION__}. Stop the old `centinel web` and start it again, then reload.</AlertDescription></Alert>}
      <Outlet />
    </Frame>
  </JobDrawerProvider>
}

/**
 * The workspace's chrome: the rail and the card a page sits in. The shell fills it once the
 * account is read; until then its skeleton does, so nothing moves when the account lands.
 */
function Frame({ nav, footer, children }: { nav?: ReactNode; footer: ReactNode; children: ReactNode }) {
  return <SidebarProvider defaultOpen={false} className="bg-ground">
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
      <SidebarContent>{nav}</SidebarContent>
      <SidebarFooter className="gap-3 px-3 pb-3">{footer}</SidebarFooter>
    </Sidebar>
    <SidebarInset className="min-w-0 bg-background md:my-2.5 md:mr-2.5 md:rounded-2xl md:border md:border-sidebar-border md:shadow-[0_1px_2px_rgba(26,23,18,0.06),0_8px_24px_rgba(26,23,18,0.05)]">
      <div className="flex h-12 items-center px-4 md:hidden"><SidebarTrigger /></div>
      <div className="min-w-0 px-5 py-6 [view-transition-name:page] md:px-10 md:py-8">{children}</div>
    </SidebarInset>
  </SidebarProvider>
}

/**
 * While the account is read: the chrome, an empty rail (the account decides its links), a
 * blank where the account sits, and the skeleton of the page being opened, if it has one.
 */
function WorkspacePending() {
  return <Frame footer={<SidebarMenu className="-mx-1"><SidebarMenuItem>
    <SidebarMenuButton size="lg" disabled tooltip="Account" className="disabled:opacity-100">
      <Skeleton className="size-8 shrink-0 rounded-full" />
      <div className="grid flex-1 text-left leading-tight">
        <span className="truncate font-semibold"><Skeleton mask="Archive reader" /></span>
        <span className="truncate text-xs"><Skeleton mask="reader@example.org" /></span>
      </div>
    </SidebarMenuButton>
  </SidebarMenuItem></SidebarMenu>}>
    <PagePending />
  </Frame>
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

/**
 * Who is signed in, and behind it everything about the workspace rather than the archive:
 * spend, documentation, colour mode, the admin page and signing out.
 */
function AccountMenu({ account }: { account: Account }) {
  const { isMobile } = useSidebar()
  const [theme, toggleTheme] = useTheme()
  const admin = account.role === 'admin'
  const who = <>
    <Portrait account={account} className="size-8 text-base" />
    <div className="grid flex-1 text-left leading-tight">
      <span className="truncate font-semibold">{account.name}</span>
      <span className="truncate text-xs text-muted-foreground">{handle(account)}</span>
    </div>
  </>
  return <SidebarMenuItem>
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <SidebarMenuButton size="lg" tooltip={account.name} className="data-[state=open]:bg-sidebar-accent data-[state=open]:text-sidebar-accent-foreground">
          {who}<ChevronsUpDown className="ml-auto size-4" />
        </SidebarMenuButton>
      </DropdownMenuTrigger>
      <DropdownMenuContent side={isMobile ? 'bottom' : 'right'} align="end" sideOffset={4} className="w-(--radix-dropdown-menu-trigger-width) min-w-56 rounded-lg">
        <DropdownMenuLabel className="p-0 font-normal"><div className="flex items-center gap-2 px-1 py-1.5 text-sm">{who}</div></DropdownMenuLabel>
        <DropdownMenuSeparator />
        <DropdownMenuGroup>
          {admin && <DropdownMenuItem asChild><Link to="/spend" search={{ metric: 'cost', days: 30 }}><CircleDollarSign />Spend</Link></DropdownMenuItem>}
          <DropdownMenuItem asChild><Link to="/docs/$" params={{ _splat: '' }}><BookOpen />Documentation</Link></DropdownMenuItem>
          {/* Kept open, so the page changes colour under the menu. */}
          <DropdownMenuItem onSelect={event => { event.preventDefault(); toggleTheme() }}>
            {theme === 'dark' ? <Sun /> : <Moon />}{theme === 'dark' ? 'Light mode' : 'Dark mode'}
          </DropdownMenuItem>
          {admin && <DropdownMenuItem asChild><Link to="/admin"><Users />Admin</Link></DropdownMenuItem>}
        </DropdownMenuGroup>
        <DropdownMenuSeparator />
        <DropdownMenuItem onSelect={signOut}><LogOut />Sign out</DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
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
