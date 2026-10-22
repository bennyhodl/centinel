import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { useQuery } from '@tanstack/react-query'
import { getRouteApi, Link } from '@tanstack/react-router'
import { AtSign, ChevronLeft, ExternalLink, FileText, Mail, PenLine, Search } from 'lucide-react'
import Graph from 'graphology'
import type { RolodexListed, RolodexShow } from './api'
import type { Canvas } from './graph-canvas'
import { labelOf, type DrawnNode, type EdgeAttributes, type Hover, type NodeAttributes, type Palette } from './graph-data'
import { paletteOf } from './graph'
import { SourceIcon } from './host-icon'
import { howNamed, personGraph, weightOf } from './rolodex-logic'
import { queries } from './queries'
import { useTheme } from './theme'
import { number, plural } from './format'
import { ErrorBox, PageHeader, SectionRule } from './ui'
import { Input } from '@/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Skeleton } from '@/components/ui/skeleton'

const listRoute = getRouteApi('/_workspace/rolodex/')

const listDetail = 'The people this archive names by linking to them. Open a name to see every source it comes up in.'

/** Radix's select has no empty value, so "every source" is spelled out. */
const EVERY = 'all'

/** A source's chip, the same slate mark a search result wears. */
const chip = 'inline-flex items-center gap-1.5 rounded-full bg-slate-soft py-0.5 pr-2 pl-1.5 text-xs font-medium text-slate shadow-[inset_0_0_0_1px_var(--slate-line)]'

/** The small uppercase label the column heads and section rules share. */
const caps = 'text-[11px] font-semibold uppercase tracking-[0.08em] text-muted-foreground'

// ── the list ─────────────────────────────────────────────────────────────────

export function RolodexPage() {
  const search = listRoute.useSearch()
  const navigate = listRoute.useNavigate()
  const list = useQuery(queries.rolodex(search))
  const sources = useQuery(queries.sourceHosts())
  // Typed into a box and written to the address a moment later, so each keystroke is not a
  // request. The draft lives only while someone types; otherwise the box shows the address,
  // so the back button or a link to another search is never written over by a stale draft.
  const [draft, setDraft] = useState<string | null>(null)
  const typed = draft ?? search.name
  useEffect(() => {
    if (draft === null) return
    const timer = setTimeout(() => void navigate({ search: previous => ({ ...previous, name: draft }), replace: true }).then(() => setDraft(null)), 250)
    return () => clearTimeout(timer)
  }, [draft, navigate])

  const data = list.data
  return <>
    <PageHeader title="Rolodex" detail={listDetail} />
    <div className="grid max-w-5xl gap-7">
      <Controls>
        <div className="relative min-w-64 flex-1">
          <Search className="pointer-events-none absolute top-1/2 left-3.5 size-4 -translate-y-1/2 text-muted-foreground" />
          <Input value={typed} onChange={event => setDraft(event.target.value)} placeholder="Search a name" aria-label="Search a name" className="h-10 bg-background pl-10 text-[15px]" />
        </div>
        <Select value={search.source || EVERY} onValueChange={value => void navigate({ search: previous => ({ ...previous, source: value === EVERY ? '' : value }), replace: true })}>
          <SelectTrigger className="w-full bg-background data-[size=default]:h-10 sm:w-52" aria-label="Source"><SelectValue /></SelectTrigger>
          <SelectContent>
            <SelectItem value={EVERY}>All sources</SelectItem>
            {Object.keys(sources.data ?? {}).sort().map(source => <SelectItem key={source} value={source}><SourceIcon source={source} size={14} />{source}</SelectItem>)}
          </SelectContent>
        </Select>
      </Controls>

      <section className="grid">
        <ListHead count={data ? plural(data.matched, 'name') : <Skeleton mask="412 names" />} />
        {list.error && <ErrorBox error={list.error} />}
        {data?.people.map(person => <PersonRow key={person.key} person={person} />)}
        {data && !data.people.length && <Nobody>
          {search.name
            ? <><span className="font-serif text-2xl text-foreground">No link names “{search.name}”</span><span>It may still be written in a document. <Link to="/rolodex/$name" params={{ name: search.name }} className="text-foreground underline underline-offset-4">Look for it in document text</Link>.</span></>
            : data.pages
              ? <><span className="font-serif text-2xl text-foreground">No link in these pages names anybody</span><span>Names come from links that point at a person: an address, an account, a page about them.</span></>
              : <><span className="font-serif text-2xl text-foreground">Nothing collected yet</span><span>Once a source has been collected, the people its pages link to appear here.</span></>}
        </Nobody>}
        {data && (data.matched > data.people.length || data.unnamed > 0 || data.unread > 0) && <Notes>
          {data.matched > data.people.length && <Note>… and {plural(data.matched - data.people.length, 'more name')} — narrow the search to reach them.</Note>}
          {data.unnamed > 0 && <Note>{plural(data.unnamed, 'address or handle', 'addresses and handles')} linked with no name beside them.</Note>}
          <Unread count={data.unread} />
        </Notes>}
      </section>
    </div>
  </>
}

/** The search box and the source filter on one soft shelf above the list. */
function Controls({ children }: { children: ReactNode }) {
  return <div className="flex flex-wrap gap-2 rounded-xl bg-parchment p-2 shadow-[inset_0_0_0_1px_var(--rule)]">{children}</div>
}

/** The column heads over the list, with the count where the first head would be. */
function ListHead({ count }: { count: ReactNode }) {
  return <div className={`flex h-8 items-center gap-6 border-b border-foreground ${caps}`}>
    <span className="flex-1">{count}</span>
    <span className="hidden w-72 sm:block">Sources</span>
    <span className="w-16 text-right">Pages</span>
  </div>
}

function PersonRow({ person }: { person: RolodexListed }) {
  const ids = [...person.emails, ...person.profiles]
  return <div className="group relative flex items-center gap-6 border-b py-3.5 hover:bg-hover">
    <span className="grid min-w-0 flex-1 gap-1">
      <Link to="/rolodex/$name" params={{ name: person.name }} className="w-fit max-w-full truncate text-[15px] leading-5 font-semibold after:absolute after:inset-0 group-hover:underline group-hover:underline-offset-4">{person.name}</Link>
      {ids.length
        ? <span className="truncate font-mono text-xs text-muted-foreground">{ids.join('  ·  ')}</span>
        : person.links > 0 && <span className="truncate text-xs text-muted-foreground">{plural(person.links, 'page about them', 'pages about them')}</span>}
      <span className="flex flex-wrap gap-1.5 pt-1 sm:hidden">{sourceChips(person.sources)}</span>
    </span>
    <span className="hidden w-72 flex-wrap gap-1.5 sm:flex">{sourceChips(person.sources)}</span>
    <span className="grid w-16 text-right tabular-nums">
      <b className="text-lg leading-6 font-semibold">{number(person.pages)}</b>
      <span className="text-[11px] text-muted-foreground">{person.pages === 1 ? 'page' : 'pages'}</span>
    </span>
  </div>
}

function sourceChips(sources: Record<string, number>) {
  return Object.entries(sources).map(([source, pages]) => <span key={source} className={chip}><SourceIcon source={source} size={12} />{source}<span className="font-mono text-[11px] opacity-70">{number(pages)}</span></span>)
}

export function RolodexSkeleton() {
  return <>
    <PageHeader title="Rolodex" detail={listDetail} />
    <div aria-busy className="grid max-w-5xl gap-7">
      <Controls>
        <div className="relative min-w-64 flex-1"><Search className="pointer-events-none absolute top-1/2 left-3.5 size-4 -translate-y-1/2 text-muted-foreground" /><Input disabled placeholder="Search a name" className="h-10 bg-background pl-10 text-[15px]" /></div>
        <Skeleton className="h-10 w-full rounded-md sm:w-52" />
      </Controls>
      <section className="grid">
        <ListHead count={<Skeleton mask="412 names" />} />
        {Array.from({ length: 6 }, (_, i) => <div key={i} className="flex items-center gap-6 border-b py-3.5">
          <span className="grid flex-1 gap-1"><span className="text-[15px] leading-5"><Skeleton mask="Guido Maniscalco" /></span><span className="font-mono text-xs"><Skeleton mask="guido.maniscalco@tampagov.net" /></span></span>
          <span className="hidden w-72 sm:block"><Skeleton className="h-5 w-20 rounded-full" /></span>
          <span className="grid w-16 text-right"><span className="text-lg leading-6"><Skeleton mask="12" /></span><span className="text-[11px]"><Skeleton mask="pages" /></span></span>
        </div>)}
      </section>
    </div>
  </>
}

/** The small print under a list: what the count leaves out. */
function Notes({ children }: { children: ReactNode }) {
  return <div className="grid gap-1.5 pt-4">{children}</div>
}

function Note({ children, flame }: { children: ReactNode; flame?: boolean }) {
  return <p className="flex items-start gap-2.5 text-[13px] leading-5 text-muted-foreground">
    <span className={`mt-[7px] size-1.5 shrink-0 rounded-full ${flame ? 'bg-flame shadow-[0_0_0_3px_var(--flame-halo)]' : 'bg-stone'}`} />
    <span>{children}</span>
  </p>
}

/** Pages nobody has read for names yet, and where that gets done. */
function Unread({ count }: { count: number }) {
  if (!count) return null
  return <Note flame>{plural(count, 'page')} {count === 1 ? 'was' : 'were'} collected before names were read, so nobody they link is here. Record its crumbs on a source’s page reads them.</Note>
}

/** An empty list: a serif line that says why, and a quieter one that says what to do. */
function Nobody({ children }: { children: ReactNode }) {
  return <div className="grid justify-items-center gap-2 px-6 py-16 text-center text-sm text-muted-foreground">{children}</div>
}

// ── one person ───────────────────────────────────────────────────────────────

export function PersonPage({ name }: { name: string }) {
  const show = useQuery(queries.rolodexPerson(name))
  if (show.error) return <><Back /><ErrorBox error={show.error} /></>
  if (!show.data) return <PersonSkeleton name={name} />
  const { person, sources, indexed, unread } = show.data
  const linked = sources.reduce((sum, a) => sum + a.linked, 0)
  const named = sources.reduce((sum, a) => sum + (a.named ?? 0), 0)
  const summary = sources.length
    ? [`Comes up in ${plural(sources.length, 'source')}`, linked ? `linked from ${plural(linked, 'page')}` : '', named ? `named in ${plural(named, 'document')}` : ''].filter(Boolean).join(' · ')
    : 'Nothing in this archive names them.'
  const also = Object.keys(person?.spellings ?? {}).filter(spelling => spelling !== person?.name)
  const emails = Object.entries(person?.emails ?? {})
  const profiles = Object.values(person?.profiles ?? {})
  const links = Object.entries(person?.links ?? {})
  const aside = emails.length + profiles.length + links.length + also.length > 0

  return <>
    <PageHeader eyebrow={<Back />} title={person?.name ?? name} detail={summary} />
    <div className="grid max-w-5xl grid-cols-1 gap-10 md:grid-cols-[1.35fr_1fr]">
      <div className="grid min-w-0 grid-cols-1 content-start gap-10">
        {sources.length > 0 && <PersonGraph show={show.data} />}
        <Section title="Sources" aside={sources.length ? 'every one the name comes up in' : undefined}>
          {sources.map(appearance => {
            const pages = person?.sources[appearance.source]
            return <div key={appearance.source} className="grid gap-2 border-b py-4">
              <div className="flex items-center gap-3">
                <SourceIcon source={appearance.source} size={22} className="text-xs" />
                <b className="text-[15px] font-semibold">{appearance.source}</b>
                <span className="ml-auto text-right text-xs text-muted-foreground">{howNamed(appearance, plural)}</span>
              </div>
              {pages && <div className="ml-[34px] grid gap-1">
                {pages.examples.map(address => <a key={address} href={address} target="_blank" rel="noreferrer" className="group inline-flex max-w-full items-center gap-1.5 font-mono text-xs text-ink-soft hover:underline hover:underline-offset-4"><span className="truncate">{address.replace(/^https?:\/\//, '')}</span><ExternalLink className="size-3 shrink-0 text-muted-foreground opacity-0 transition-opacity duration-(--motion-micro) group-hover:opacity-100" /></a>)}
                {pages.pages > pages.examples.length && <span className="text-xs text-muted-foreground">… and {plural(pages.pages - pages.examples.length, 'more page')}</span>}
              </div>}
            </div>
          })}
          {!sources.length && <Nobody>
            <span className="font-serif text-2xl text-foreground">Nobody by this name</span>
            <span>No link carries this name and no indexed document holds it, spelled this way.</span>
          </Nobody>}
          {(!indexed || unread > 0) && <Notes>
            {!indexed && <Note flame>Document text was not searched: this archive has no index yet.</Note>}
            <Unread count={unread} />
          </Notes>}
        </Section>
      </div>
      <div className="grid min-w-0 grid-cols-1 content-start gap-8">
          {emails.length > 0 && <Section title="Addresses" aside={plural(emails.length, 'address', 'addresses')}>
            {emails.map(([email, pages]) => <Row key={email} icon={<Mail />} href={`mailto:${email}`} label={email} detail={plural(pages, 'page')} />)}
          </Section>}
          {profiles.length > 0 && <Section title="Accounts" aside={plural(profiles.length, 'account')}>
            {profiles.map(profile => <Row key={`${profile.platform}:${profile.handle}`} icon={<AtSign />} href={profile.url} label={`${profile.platform} · ${profile.handle}`} detail={plural(profile.pages, 'page')} />)}
          </Section>}
          {links.length > 0 && <Section title="Pages about them" aside={plural(links.length, 'page')}>
            {links.map(([url, pages]) => <Row key={url} icon={<FileText />} href={url} label={url.replace(/^https?:\/\//, '')} detail={plural(pages, 'page')} />)}
          </Section>}
          {also.length > 0 && <Section title="Also written" aside={plural(also.length, 'spelling')}>
            {also.map(spelling => <Row key={spelling} icon={<PenLine />} label={spelling} detail={plural(person?.spellings[spelling] ?? 0, 'link')} sans />)}
          </Section>}
          {!aside && sources.length > 0 && <Section title="Addresses, accounts, pages">
            <p className="py-3 text-[13px] leading-5 text-muted-foreground">No link carries an address, account or page for them: the name is only written in document text.</p>
          </Section>}
      </div>
    </div>
  </>
}

export function PersonSkeleton({ name }: { name: string }) {
  return <div aria-busy>
    <PageHeader eyebrow={<Back />} title={name} />
    <div className="grid max-w-5xl grid-cols-1 gap-10 md:grid-cols-[1.35fr_1fr]">
      <div className="grid min-w-0 grid-cols-1 content-start gap-10">
      <div className={`${frame} grid place-items-center`}>
        <svg viewBox="0 0 320 200" className="w-72 opacity-60" aria-hidden>
          <g className="stroke-rule" strokeWidth="2"><line x1="160" y1="100" x2="100" y2="40" /><line x1="160" y1="100" x2="220" y2="40" /><line x1="160" y1="100" x2="80" y2="160" /><line x1="160" y1="100" x2="160" y2="165" /><line x1="160" y1="100" x2="240" y2="160" /></g>
          <circle cx="160" cy="100" r="15" className="animate-pulse fill-canvas stroke-track" strokeWidth="3" />
          <g className="fill-track"><circle cx="100" cy="40" r="9" /><circle cx="220" cy="40" r="8" /><circle cx="80" cy="160" r="6" /><circle cx="160" cy="165" r="6" /><circle cx="240" cy="160" r="6" /></g>
        </svg>
      </div>
        <Section title="Sources">{Array.from({ length: 2 }, (_, i) => <div key={i} className="grid gap-2 border-b py-4"><span className="text-[15px]"><Skeleton mask="tampa · linked from 3 pages · named in 38 documents" /></span><span className="ml-[34px] font-mono text-xs"><Skeleton mask="www.tampa.gov/city-council" /></span></div>)}</Section>
      </div>
      <div className="grid min-w-0 grid-cols-1 content-start gap-8">
        <Section title="Addresses"><div className="border-b py-2.5 font-mono text-xs"><Skeleton mask="lviera@tampagov.net" /></div></Section>
      </div>
    </div>
  </div>
}

function Back() {
  return <Link to="/rolodex" search={{}} className="inline-flex items-center gap-1 text-[13px] text-muted-foreground hover:text-foreground"><ChevronLeft className="size-3.5" />Rolodex</Link>
}

function Section({ title, aside, children }: { title: string; aside?: ReactNode; children: ReactNode }) {
  return <section className="grid min-w-0 grid-cols-1 content-start"><SectionRule aside={aside}>{title}</SectionRule>{children}</section>
}

/** One address, account, page or spelling: its mark, the thing itself, and how many pages carry it. */
function Row({ icon, label, detail, href, sans }: { icon: ReactNode; label: string; detail: string; href?: string; sans?: boolean }) {
  const body = <>
    <span className="grid size-5 shrink-0 place-items-center text-stone [&_svg]:size-3.5">{icon}</span>
    <span className={`min-w-0 flex-1 truncate ${sans ? 'text-sm' : 'font-mono text-xs'}`} title={label}>{label}</span>
    <span className="shrink-0 text-xs text-muted-foreground">{detail}</span>
    {href && <ExternalLink className="size-3 shrink-0 text-muted-foreground opacity-0 transition-opacity duration-(--motion-micro) group-hover:opacity-100" />}
  </>
  const classes = 'group flex w-full items-center gap-2.5 border-b py-2.5'
  return href ? <a href={href} target="_blank" rel="noreferrer" className={`${classes} hover:bg-hover`}>{body}</a> : <div className={classes}>{body}</div>
}

// ── the graph ────────────────────────────────────────────────────────────────

/** The graph's frame: the Graph page's dotted canvas, as a card on this page. */
const frame = 'relative h-72 overflow-hidden rounded-2xl bg-canvas bg-[radial-gradient(var(--dot)_1.4px,transparent_1.4px)] [background-size:18px_18px] shadow-[0_0_0_1px_var(--rule),0_10px_30px_rgba(26,23,18,0.05)] md:h-80'
const card = 'rounded-xl bg-background/95 shadow-[0_0_0_1px_var(--rule),0_4px_14px_rgba(26,23,18,0.06)]'

/**
 * The person, the sources around them and what their links point at, on the Graph page's
 * canvas. Hover for a node's numbers; click an address, account or page to open it.
 */
function PersonGraph({ show }: { show: RolodexShow }) {
  const [theme] = useTheme()
  const [palette, setPalette] = useState<Palette | null>(null)
  useEffect(() => setPalette(paletteOf()), [theme])
  const shown = useMemo(() => personGraph(show), [show])
  const nodes = useMemo(() => new Map(shown.nodes.map(node => [node.id, node])), [shown])
  const container = useRef<HTMLDivElement>(null)
  const [hover, setHover] = useState<Hover | null>(null)
  const openable = shown.nodes.some(node => node.kind === 'identifier')

  useEffect(() => {
    const element = container.current
    if (!element || !palette) return
    setHover(null)
    let canvas: Canvas | null = null
    let gone = false
    const open = (id: string) => { const node = nodes.get(id); if (node?.kind === 'identifier') window.open(node.url, '_blank', 'noreferrer') }
    void import('./graph-canvas').then(({ mountGraph }) => {
      if (gone) return
      canvas = mountGraph(element, new Graph<NodeAttributes, EdgeAttributes>({ multi: false, type: 'directed' }), palette, { hover: setHover, click: open, settling: () => {} })
      canvas.show(shown)
    })
    return () => { gone = true; canvas?.kill() }
  }, [palette, shown, nodes])

  const hovered = hover?.node !== undefined ? nodes.get(hover.node) : hover?.ends ? nodes.get(hover.ends[0]) : undefined
  return <figure className="grid gap-3">
    <div className={frame}>
      <div ref={container} className="absolute inset-0" />
      {hover && hovered && <div className={`pointer-events-none absolute z-20 grid max-w-80 gap-0.5 px-3.5 py-2.5 ${card}`} style={{ left: hover.flip ? hover.x - 14 : hover.x + 14, top: hover.y + 14, transform: hover.flip ? 'translateX(-100%)' : undefined }}>
        <b className="truncate text-sm">{labelOf(hovered)}</b>
        <span className="truncate text-xs text-muted-foreground">{lineOf(hovered)}</span>
      </div>}
    </div>
    <figcaption className="grid gap-1.5 px-1 text-xs text-muted-foreground">
      <span className="flex flex-wrap items-center gap-x-4 gap-y-1.5">
        <span className="inline-flex items-center gap-1.5"><span className="size-3 rounded-full border-[1.5px] border-foreground bg-canvas" />Person</span>
        <span className="inline-flex items-center gap-1.5"><span className="size-3 rounded-full bg-foreground" />Source</span>
        {openable && <span className="inline-flex items-center gap-1.5"><span className="size-3 rounded-full bg-stone" />Address, account, page about them</span>}
        <span className="inline-flex items-center gap-1.5"><span className="h-0.5 w-5 rounded-full bg-rule" /><span className="h-1 w-5 rounded-full bg-rule" />How often it names them</span>
      </span>
      <span className="hidden sm:block">{openable ? 'Hover a node for its numbers · click an address, account or page to open it' : 'Hover a node for its numbers'}</span>
    </figcaption>
  </figure>
}

/** What a node is, in one line under its name. */
function lineOf(node: DrawnNode): string {
  switch (node.kind) {
    case 'person': return node.pages ? `linked from ${plural(node.pages, 'page')}` : 'no link names them'
    case 'appearance': return howNamed(node, plural) || plural(weightOf(node), 'page')
    case 'identifier': return `${node.what} · ${plural(node.pages, 'page')} · click to open`
    case 'source': return plural(node.addresses, 'address', 'addresses')
    case 'host': return plural(node.pages, 'page')
  }
}
