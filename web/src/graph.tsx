import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { getRouteApi, Link, useNavigate } from '@tanstack/react-router'
import { Ban, ExternalLink, FlaskConical, Search, Undo2 } from 'lucide-react'
import Graph from 'graphology'
import { api, originalUrl, type GraphEdge, type GraphFilter, type GraphNode, type GraphView } from './api'
import type { Canvas } from './graph-canvas'
import { edgeTitle, labelOf, searchNodes, type Drawn, type EdgeAttributes, type Hover, type NodeAttributes, type Palette } from './graph-data'
import { HostIcon, SourceIcon } from './host-icon'
import { queries } from './queries'
import { useTheme } from './theme'
import { compact, number, plural } from './format'
import { ErrorBox, SectionRule, Spinner } from './ui'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Sheet, SheetContent, SheetDescription, SheetTitle } from '@/components/ui/sheet'
import { Skeleton } from '@/components/ui/skeleton'

const route = getRouteApi('/_workspace/_admin/graph')

/** The page's frame: the canvas takes the whole card, as the Classify page does. */
const frame = 'relative -mx-5 -my-6 h-[calc(100svh-3rem)] overflow-hidden bg-canvas bg-[radial-gradient(var(--dot)_1.4px,transparent_1.4px)] [background-size:18px_18px] md:-mx-10 md:-my-8 md:h-[calc(100svh-1.25rem-2px)] md:rounded-2xl'
const card = 'rounded-xl bg-background/95 shadow-[0_0_0_1px_var(--rule),0_4px_14px_rgba(26,23,18,0.06)]'

/** "At least N pages" choices. Steps rather than a free number, so one filter change is one read. */
const minPagesOptions = [1, 2, 5, 10, 25, 100]

/** The canvas is drawn in the page's own tokens, read once the page is on screen. */
function paletteOf(): Palette {
  const style = getComputedStyle(document.documentElement)
  const token = (name: string, fallback: string) => style.getPropertyValue(name).trim() || fallback
  return {
    ink: token('--foreground', '#1A1712'),
    paper: token('--canvas', '#FBF8F1'),
    stone: token('--stone', '#B9AE98'),
    track: token('--track', '#EFE9DC'),
    rule: token('--rule', '#E2DACB'),
    strong: token('--ink-soft', '#3A352D'),
  }
}


/**
 * Sources and the hosts their pages link to, drawn as a graph. Each edge is as thick as
 * the log of the pages carrying it. Hover an edge for its weight, a node for its card;
 * click a node and the drawer opens on it.
 */
export function GraphPage() {
  const search = route.useSearch()
  const navigate = route.useNavigate()
  const filter: GraphFilter = { min_pages: search.min, hide_refused: search.refused }
  const view = useQuery(queries.graph(filter))
  const [theme] = useTheme()
  const [palette, setPalette] = useState<Palette | null>(null)
  useEffect(() => setPalette(paletteOf()), [theme])

  const container = useRef<HTMLDivElement>(null)
  // One graphology graph for the page's life: the canvas draws it, the layout moves it,
  // and each view rewrites its contents in place, so positions survive a filter change.
  const drawn = useRef<Drawn | null>(null)
  const canvas = useRef<Canvas | null>(null)
  const latest = useRef<GraphView | undefined>(undefined)
  const [hover, setHover] = useState<Hover | null>(null)
  const [selected, setSelected] = useState<string | null>(null)
  const [settling, setSettling] = useState(false)
  const nodes = useMemo(() => new Map((view.data?.nodes ?? []).map(node => [node.id, node])), [view.data])

  // The renderer is loaded in the browser alone (see `graph-canvas.ts`) once the palette
  // is known, and again when the theme changes it.
  useEffect(() => {
    const element = container.current
    if (!element || !palette) return
    const graph = (drawn.current ??= new Graph<NodeAttributes, EdgeAttributes>({ multi: false, type: 'directed' }))
    // A card left over from the renderer being replaced would name a node nothing lights.
    setHover(null)
    let gone = false
    void import('./graph-canvas').then(({ mountGraph }) => {
      if (gone) return
      canvas.current = mountGraph(element, graph, palette, { hover: setHover, click: setSelected, settling: setSettling })
      if (latest.current) canvas.current.show(latest.current)
    })
    return () => { gone = true; canvas.current?.kill(); canvas.current = null }
  }, [palette])

  useEffect(() => {
    latest.current = view.data
    if (view.data) canvas.current?.show(view.data)
  }, [view.data])

  const focus = (id: string) => canvas.current?.focus(id)

  const setFilter = (patch: Partial<{ min: number; refused: boolean }>) => void navigate({ search: previous => ({ ...previous, ...patch }), replace: true })
  const totals = view.data?.totals
  const shown = view.data ? { hosts: view.data.nodes.filter(node => node.kind === 'host').length, edges: view.data.edges.length } : null
  const chosen = selected ? nodes.get(selected) : undefined

  return <div className={frame}>
    <div ref={container} className="absolute inset-0" />
    {hover && view.data && <HoverCard hover={hover} nodes={nodes} edges={view.data.edges} />}

    <div className="pointer-events-none absolute inset-x-4 top-4 flex flex-wrap items-start justify-between gap-3">
      <div className="grid justify-items-start gap-3 [&>*]:pointer-events-auto">
        <div className={`flex flex-wrap items-center gap-3 px-4 py-2.5 ${card}`}>
          <span className="font-serif text-[28px] leading-none">Graph</span>
          <span className="h-6 w-px bg-rule" />
          <span className="text-[13px] text-muted-foreground">
            {totals && shown
              ? <>{plural(totals.sources, 'source')} · {shown.hosts === totals.hosts ? plural(totals.hosts, 'host') : `${number(shown.hosts)} of ${plural(totals.hosts, 'host')}`} · {shown.edges === totals.edges ? plural(totals.edges, 'edge') : `${number(shown.edges)} of ${plural(totals.edges, 'edge')}`} · {compact(totals.links)} links · {number(totals.open)} open</>
              : <Skeleton mask="16 sources · 461 of 2,847 hosts · 616 edges · 625k links" />}
          </span>
        </div>
        <div className={`flex flex-wrap items-center gap-3 px-4 py-2.5 ${card}`}>
          <NodeSearch nodes={view.data?.nodes ?? []} onPick={focus} />
          <span className="h-6 w-px bg-rule" />
          <span className="text-xs text-muted-foreground">At least</span>
          <Select value={String(search.min)} onValueChange={value => setFilter({ min: Number(value) })}>
            <SelectTrigger className="h-8 w-32 text-[13px]"><SelectValue /></SelectTrigger>
            <SelectContent>{minPagesOptions.map(option => <SelectItem key={option} value={String(option)}>{plural(option, 'page')}</SelectItem>)}</SelectContent>
          </Select>
          <Button size="sm" variant={search.refused ? 'default' : 'outline'} aria-pressed={search.refused} onClick={() => setFilter({ refused: !search.refused })}><Ban />{search.refused ? 'Refused hidden' : 'Hide refused'}</Button>
        </div>
        {view.error && <ErrorBox error={view.error} />}
      </div>
      <Legend />
    </div>

    <div className="pointer-events-none absolute inset-x-4 bottom-4 flex flex-wrap items-end justify-between gap-3">
      <span />
      <span className={`rounded-full bg-background/90 px-3 py-1 text-xs text-muted-foreground shadow-[0_0_0_1px_var(--rule)]`}>Drag to move · scroll to zoom · hover an edge for its weight · click a node for its card</span>
      <span className="grid justify-items-end gap-2 text-xs">
        {view.data?.truncated && <span className="rounded-full bg-background/90 px-3 py-1 text-muted-foreground shadow-[0_0_0_1px_var(--rule)]">Showing the strongest {number(view.data.edges.length)} edges; raise the page floor to see the rest</span>}
        {(settling || view.isFetching) && <span className="inline-flex items-center gap-2 rounded-full bg-background/90 px-3 py-1 text-flame-ink shadow-[0_0_0_1px_var(--rule)]"><span className="size-1.5 rounded-full bg-flame shadow-[0_0_0_3px_var(--flame-halo)]" />{view.isFetching ? 'Reading the ledgers' : 'Settling the layout'}</span>}
      </span>
    </div>

    {view.data && !view.data.edges.length && <div className="pointer-events-none absolute inset-0 grid place-items-center">
      <div className={`pointer-events-auto grid max-w-sm justify-items-center gap-2 p-8 text-center ${card}`}>
        <span className="font-serif text-2xl">{totals?.edges ? 'Nothing clears the floor' : 'Nothing links anywhere yet'}</span>
        <p className="text-sm text-muted-foreground">{totals?.edges ? 'Lower the page floor, or show refused hosts, to see the edges the filter hides.' : 'Edges come from each source’s crumb ledger. A source collected before the ledger existed has one once you record its crumbs on its page.'}</p>
      </div>
    </div>}

    {chosen && view.data && <NodeDrawer node={chosen} view={view.data} nodes={nodes} onClose={() => setSelected(null)} onFocus={focus} />}
  </div>
}

/** The canvas before the graph arrives: the same frame and chrome, with blanks where the counts go. */
export function GraphSkeleton() {
  return <div aria-busy className={frame}>
    <div className="absolute inset-x-4 top-4 flex flex-wrap items-start justify-between gap-3">
      <div className="grid justify-items-start gap-3">
        <div className={`flex items-center gap-3 px-4 py-2.5 ${card}`}>
          <span className="font-serif text-[28px] leading-none">Graph</span>
          <span className="h-6 w-px bg-rule" />
          <span className="text-[13px]"><Skeleton mask="16 sources · 461 of 2,847 hosts · 616 edges · 625k links" /></span>
        </div>
        <div className={`flex items-center gap-3 px-4 py-2.5 ${card}`}>
          <Input disabled placeholder="Find a source or host" className="h-8 w-64 text-[13px]" />
          <span className="h-6 w-px bg-rule" />
          <span className="text-xs text-muted-foreground">At least</span>
          <Skeleton className="h-8 w-32 rounded-md" />
          <Button size="sm" variant="outline" disabled><Ban />Hide refused</Button>
        </div>
      </div>
      <Legend />
    </div>
    <div className="absolute inset-0 grid place-items-center">
      <svg viewBox="0 0 320 200" className="w-80 opacity-60" aria-hidden>
        <g className="stroke-rule" strokeWidth="2"><line x1="160" y1="100" x2="60" y2="50" /><line x1="160" y1="100" x2="250" y2="40" /><line x1="160" y1="100" x2="90" y2="160" /><line x1="160" y1="100" x2="270" y2="150" /></g>
        <circle cx="160" cy="100" r="14" className="animate-pulse fill-track" />
        <g className="fill-canvas stroke-track" strokeWidth="2"><circle cx="60" cy="50" r="7" /><circle cx="250" cy="40" r="9" /><circle cx="90" cy="160" r="6" /><circle cx="270" cy="150" r="8" /></g>
      </svg>
    </div>
  </div>
}

function Legend() {
  return <div className={`pointer-events-auto flex items-center gap-4 px-3 py-2 text-xs text-muted-foreground ${card}`}>
    <span className="inline-flex items-center gap-1.5"><span className="size-3 rounded-full bg-foreground" />Source</span>
    <span className="inline-flex items-center gap-1.5"><span className="size-3 rounded-full border-[1.5px] border-foreground bg-canvas" />Host</span>
    <span className="inline-flex items-center gap-1.5"><span className="size-3 rounded-full border-[1.5px] border-stone bg-track" />Refused</span>
    <span className="inline-flex items-center gap-1.5"><span className="h-0.5 w-5 rounded-full bg-rule" /><span className="h-1 w-5 rounded-full bg-rule" />Pages</span>
  </div>
}

/** Find a source or a host by name, and bring it to the centre. */
function NodeSearch({ nodes, onPick }: { nodes: GraphNode[]; onPick: (id: string) => void }) {
  const [query, setQuery] = useState('')
  const [open, setOpen] = useState(false)
  const found = searchNodes(nodes, query)
  const pick = (node: GraphNode) => { onPick(node.id); setQuery(labelOf(node)); setOpen(false) }
  return <div className="relative" onBlur={event => { if (!event.currentTarget.contains(event.relatedTarget as Node)) setOpen(false) }}>
    <Search className="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-muted-foreground" />
    <Input value={query} onChange={event => { setQuery(event.target.value); setOpen(true) }} onFocus={() => setOpen(true)} onKeyDown={event => { if (event.key === 'Enter' && found[0]) pick(found[0]); if (event.key === 'Escape') setOpen(false) }} placeholder="Find a source or host" aria-label="Find a source or host" className="h-8 w-64 pl-8 text-[13px]" />
    {open && found.length > 0 && <div role="listbox" className="absolute top-full left-0 z-30 mt-2 grid w-80 rounded-lg border bg-popover p-1 shadow-md">
      {found.map(node => <button type="button" role="option" aria-selected={false} key={node.id} onMouseDown={event => event.preventDefault()} onClick={() => pick(node)} className="flex items-center gap-2.5 rounded px-2 py-1.5 text-left text-[13px] hover:bg-accent">
        <NodeIcon node={node} size={18} />
        <span className="min-w-0 flex-1 truncate">{labelOf(node)}</span>
        <span className="shrink-0 text-xs text-muted-foreground">{node.kind === 'source' ? plural(node.addresses, 'address', 'addresses') : plural(node.pages, 'page')}</span>
      </button>)}
    </div>}
  </div>
}

function NodeIcon({ node, size, className }: { node: GraphNode; size: number; className?: string }) {
  return node.kind === 'source' ? <SourceIcon source={node.source} size={size} className={className} /> : <HostIcon host={node.host} size={size} className={className} />
}

const standingLabel = { open: 'Open', ignored: 'Refused', promoted: 'Already a source' } as const

/** What a node is, in one line under its name. */
function nodeLine(node: GraphNode) {
  return node.kind === 'source'
    ? `Source · ${node.acquisition} · ${plural(node.addresses, 'address', 'addresses')}${node.enabled ? '' : ' · disabled'}`
    : `Host · ${plural(node.pages, 'page')} · ${plural(node.links, 'link')} · ${plural(node.sources, 'source')}${node.standing === 'ignored' ? ' · refused' : ''}`
}

/** The card beside the pointer: a node's icon, name and counts, or an edge's weight with both ends named. */
function HoverCard({ hover, nodes, edges }: { hover: Hover; nodes: Map<string, GraphNode>; edges: GraphEdge[] }) {
  const style = { left: hover.flip ? hover.x - 14 : hover.x + 14, top: hover.y + 14, transform: hover.flip ? 'translateX(-100%)' : undefined }
  if (hover.node !== undefined) {
    const node = nodes.get(hover.node)
    if (!node) return null
    return <div className={`pointer-events-none absolute z-20 flex max-w-80 items-center gap-3 px-3.5 py-2.5 ${card}`} style={style}>
      <NodeIcon node={node} size={28} className="text-sm" />
      <span className="grid min-w-0 gap-0.5"><b className="truncate text-sm">{labelOf(node)}</b><span className="truncate text-xs text-muted-foreground">{nodeLine(node)}</span></span>
    </div>
  }
  if (hover.ends) {
    const [from, to] = hover.ends
    const edge = edges.find(candidate => candidate.from === from && candidate.to === to)
    if (!edge) return null
    const title = edgeTitle(edge, nodes)
    return <div className={`pointer-events-none absolute z-20 grid max-w-80 gap-0.5 px-3.5 py-2.5 ${card}`} style={style}>
      <b className="text-sm">{plural(title.pages, 'page')} · {plural(title.links, 'link')}</b>
      <span className="truncate text-xs text-muted-foreground">{title.from} → {title.to}</span>
    </div>
  }
  return null
}

/**
 * One node's drawer: a host's card from the ledgers, or a source's from the graph it is
 * already part of. The actions a crumb has elsewhere — investigate, refuse, allow — plus
 * the source's own page.
 */
function NodeDrawer({ node, view, nodes, onClose, onFocus }: { node: GraphNode; view: GraphView; nodes: Map<string, GraphNode>; onClose: () => void; onFocus: (id: string) => void }) {
  const navigate = useNavigate()
  const investigate = useMutation({
    mutationFn: (url: string) => api.tryAddress(url),
    onSuccess: ({ id }) => navigate({ to: '/sources/try/$id', params: { id } }),
  })
  const jump = (id: string) => { onClose(); onFocus(id) }
  return <Sheet open onOpenChange={open => !open && onClose()}>
    <SheetContent side="right" className="flex w-full flex-col gap-0 p-0 sm:max-w-[560px]">
      {node.kind === 'host'
        ? <HostCard node={node} view={view} nodes={nodes} onJump={jump} onInvestigate={() => investigate.mutate(`https://${node.host}/`)} investigating={investigate.isPending} error={investigate.error} />
        : <SourceCard node={node} view={view} nodes={nodes} onJump={jump} />}
    </SheetContent>
  </Sheet>
}

function HostCard({ node, view, nodes, onJump, onInvestigate, investigating, error }: { node: Extract<GraphNode, { kind: 'host' }>; view: GraphView; nodes: Map<string, GraphNode>; onJump: (id: string) => void; onInvestigate: () => void; investigating: boolean; error: Error | null }) {
  const client = useQueryClient()
  const detail = useQuery(queries.graphHost(node.host))
  const refresh = () => Promise.all([client.invalidateQueries({ queryKey: ['graph'] }), client.invalidateQueries({ queryKey: ['graph-host', node.host] }), client.invalidateQueries({ queryKey: ['source-crumbs'] })])
  const refuse = useMutation({ mutationFn: () => api.refuseHost(node.host), onSuccess: refresh })
  const allow = useMutation({ mutationFn: () => api.allowHost(node.host), onSuccess: refresh })
  const standing = detail.data?.standing ?? node.standing
  const refused = standing === 'ignored'
  const sourceNode = (name: string) => view.nodes.find(other => other.kind === 'source' && other.source === name)
  const linkedFrom = detail.data?.sources ?? view.edges.filter(edge => edge.to === node.id).flatMap(edge => { const from = nodes.get(edge.from); return from?.kind === 'source' ? [{ source: from.source, pages: edge.pages, links: edge.links, example: '' }] : [] })
  return <>
    <CardHeader eyebrow={`Host · ${standingLabel[standing]}`} tone={refused ? 'text-muted-foreground' : 'text-flame-ink'} icon={<HostIcon host={node.host} size={32} className="text-base" />} title={node.host} href={detail.data?.example}>
      {detail.data ? <>{plural(detail.data.pages, 'page')} · {plural(detail.data.links, 'link')} · {plural(detail.data.sources.length, 'source')}{detail.data.note ? ` · ${detail.data.note}` : ''}</> : <Skeleton mask="11,442 pages · 198,470 links · 3 sources" />}
    </CardHeader>
    <div className="grid min-h-0 flex-1 content-start gap-7 overflow-y-auto px-6 py-5">
      <Section title="Linked from" aside={plural(linkedFrom.length, 'source')}>
        {linkedFrom.map(from => { const found = sourceNode(from.source); return <Row key={from.source} onClick={found ? () => onJump(found.id) : undefined} icon={<SourceIcon source={from.source} size={20} />} label={from.source} detail={`${plural(from.pages, 'page')} · ${plural(from.links, 'link')}`} /> })}
      </Section>
      <Section title="Where it points" aside={detail.data ? `${plural(detail.data.paths.length + detail.data.paths_more, 'path')}` : ''}>
        {detail.isPending && <Rows />}
        {detail.data?.paths.map(path => <Row key={path.path} mono label={path.path} detail={plural(path.pages, 'page')} />)}
        {detail.data && detail.data.paths_more > 0 && <More count={detail.data.paths_more} noun="path" />}
      </Section>
      <Section title="Referring pages" aside={detail.data ? plural(detail.data.referrers.length + detail.data.referrers_more, 'page') : ''}>
        {detail.isPending && <Rows />}
        {detail.data?.referrers.map(page => <Row key={`${page.source}:${page.address}`} mono label={page.address} detail={page.source} href={page.blob ? originalUrl(page.blob, page.source) : page.address} />)}
        {detail.data && detail.data.referrers_more > 0 && <More count={detail.data.referrers_more} noun="page" />}
      </Section>
      {detail.error && <ErrorBox error={detail.error} />}
      {(refuse.error || allow.error || error) && <ErrorBox error={(refuse.error || allow.error || error) as Error} />}
    </div>
    <div className="flex items-center gap-2 border-t px-6 py-4">
      <span className="flex-1 text-[13px] text-muted-foreground">{refused ? 'Refused: no pass offers it again.' : standing === 'promoted' ? 'A source collects this host.' : 'Open: nothing has been decided.'}</span>
      {refused
        ? <Button variant="outline" disabled={allow.isPending} onClick={() => allow.mutate()}>{allow.isPending ? <Spinner /> : <Undo2 />}Allow</Button>
        : standing === 'open' && <Button variant="outline" disabled={refuse.isPending} onClick={() => refuse.mutate()}>{refuse.isPending ? <Spinner /> : <Ban />}Refuse</Button>}
      <Button disabled={investigating} onClick={onInvestigate}>{investigating ? <Spinner /> : <FlaskConical />}Investigate</Button>
    </div>
  </>
}

function SourceCard({ node, view, nodes, onJump }: { node: Extract<GraphNode, { kind: 'source' }>; view: GraphView; nodes: Map<string, GraphNode>; onJump: (id: string) => void }) {
  const out = view.edges.filter(edge => edge.from === node.id)
  const into = view.edges.filter(edge => edge.to === node.id)
  const name = (id: string) => { const other = nodes.get(id); return other ? labelOf(other) : id }
  const shown = 40
  return <>
    <CardHeader eyebrow={`Source · ${node.acquisition}${node.enabled ? '' : ' · disabled'}`} icon={<SourceIcon source={node.source} size={32} className="text-base" />} title={node.source} href={node.target}>
      {plural(node.addresses, 'address', 'addresses')} · links to {plural(out.length, 'host')} · linked from {plural(into.length, 'source')}
    </CardHeader>
    <div className="grid min-h-0 flex-1 content-start gap-7 overflow-y-auto px-6 py-5">
      <Section title="Links to" aside={`${plural(out.length, 'host')} in view`}>
        {out.slice(0, shown).map(edge => { const other = nodes.get(edge.to); return <Row key={edge.to} onClick={() => onJump(edge.to)} icon={other ? <NodeIcon node={other} size={20} /> : undefined} label={name(edge.to)} detail={`${plural(edge.pages, 'page')} · ${plural(edge.links, 'link')}`} /> })}
        {out.length > shown && <More count={out.length - shown} noun="host" />}
        {!out.length && <p className="py-2 text-sm text-muted-foreground">No edge from this source clears the filter.</p>}
      </Section>
      <Section title="Linked from" aside={plural(into.length, 'source')}>
        {into.map(edge => { const other = nodes.get(edge.from); return <Row key={edge.from} onClick={() => onJump(edge.from)} icon={other ? <NodeIcon node={other} size={20} /> : undefined} label={name(edge.from)} detail={`${plural(edge.pages, 'page')} · ${plural(edge.links, 'link')}`} /> })}
        {!into.length && <p className="py-2 text-sm text-muted-foreground">No other source in view links here.</p>}
      </Section>
    </div>
    <div className="flex items-center gap-2 border-t px-6 py-4">
      <span className="flex-1 text-[13px] text-muted-foreground">{node.host ?? node.target}</span>
      <Link to="/sources/$id" params={{ id: node.source }} className="inline-flex h-[34px] items-center gap-2 rounded-lg bg-foreground px-3.5 text-[13px] font-medium text-parchment">Open source</Link>
    </div>
  </>
}

/** The top of a node's card: what it is, its mark and name, its numbers, and where it lives. */
function CardHeader({ eyebrow, tone = 'text-muted-foreground', icon, title, href, children }: { eyebrow: string; tone?: string; icon: ReactNode; title: string; href?: string; children: ReactNode }) {
  return <div className="grid gap-3.5 border-b px-6 pt-6 pb-5">
    <span className={`text-[11px] font-semibold uppercase tracking-[0.08em] ${tone}`}>{eyebrow}</span>
    <SheetTitle className="flex items-center gap-3 font-serif text-[30px] leading-8 font-normal">{icon}<span className="min-w-0 truncate">{title}</span></SheetTitle>
    <SheetDescription className="text-[13px]">{children}</SheetDescription>
    {href && <a href={href} target="_blank" rel="noreferrer" className="inline-flex max-w-full items-center gap-1.5 truncate font-mono text-xs text-muted-foreground hover:underline"><ExternalLink className="size-3 shrink-0" />{href}</a>}
  </div>
}

function Section({ title, aside, children }: { title: string; aside?: ReactNode; children: ReactNode }) {
  // `min-w-0`, or a long address in a row widens the section past the drawer and pushes
  // every row's count out of sight.
  return <section className="grid min-w-0"><SectionRule aside={aside}>{title}</SectionRule>{children}</section>
}

function Row({ icon, label, detail, mono, href, onClick }: { icon?: ReactNode; label: string; detail: string; mono?: boolean; href?: string; onClick?: () => void }) {
  const body = <>
    {icon}
    <span className={`min-w-0 flex-1 truncate ${mono ? 'font-mono text-xs' : 'text-sm font-medium'}`} title={label}>{label}</span>
    <span className="shrink-0 text-xs text-muted-foreground">{detail}</span>
  </>
  const classes = 'flex w-full items-center gap-2.5 border-b py-2 text-left'
  if (href) return <a href={href} target="_blank" rel="noreferrer" className={`${classes} hover:bg-hover`}>{body}</a>
  if (onClick) return <button type="button" onClick={onClick} className={`${classes} hover:bg-hover`}>{body}</button>
  return <div className={classes}>{body}</div>
}

function Rows() {
  return <div aria-busy>{Array.from({ length: 3 }, (_, i) => <div key={i} className="flex items-center gap-2.5 border-b py-2"><span className="flex-1 font-mono text-xs"><Skeleton mask="/Civil/Records/Search" /></span><span className="text-xs"><Skeleton mask="412 pages" /></span></div>)}</div>
}

function More({ count, noun }: { count: number; noun: string }) {
  return <p className="py-2 text-xs text-muted-foreground">… and {plural(count, noun)} more</p>
}

