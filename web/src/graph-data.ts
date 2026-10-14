import Graph from 'graphology'
import type { GraphEdge, GraphNode, GraphView } from './api'

/**
 * What the Graph page draws, shaped from what the server sends. Pure: no canvas, no
 * React, so the sizes, colours and the search can be tested as the functions they are.
 */

/** The colours the canvas is drawn in, read off the page's own tokens at mount. */
export type Palette = {
  /** Sources are filled in this, and it is the border of an open host. */
  ink: string
  /** An open host's fill: the page's ground, so it reads as outlined. */
  paper: string
  /** A refused host's border and label. */
  stone: string
  /** A refused host's fill. */
  track: string
  /** An edge at rest. */
  rule: string
  /** An edge under the pointer, or beside a node under it. */
  strong: string
}

export type NodeAttributes = {
  x: number
  y: number
  size: number
  color: string
  borderColor: string
  label: string
  /** The node-border program draws every node; see `graph.tsx`. */
  type: 'border'
  /** Sources are always labelled; a host earns its label by size. */
  forceLabel: boolean
  node: GraphNode
}

export type EdgeAttributes = { size: number; color: string; pages: number; links: number }

export type Drawn = Graph<NodeAttributes, EdgeAttributes>

/**
 * What is under the pointer, where, and which nodes it lights: a node, or an edge with
 * both its ends. `flip` puts the card to the pointer's left, near the right edge.
 */
export type Hover = { node?: string; edge?: string; ends?: [string, string]; lit: Set<string>; x: number; y: number; flip?: boolean }

export const labelOf = (node: GraphNode) => node.kind === 'source' ? node.source : node.host

/** A source by how many addresses it holds, a host by how many pages link to it. Log, so a city and a village both fit. */
export function sizeOf(node: GraphNode): number {
  return node.kind === 'source'
    ? 8 + 3 * Math.log10(node.addresses + 1)
    : 3 + 1.4 * Math.log10(node.pages + 1)
}

/** Edge width grows with the log of the pages carrying it: one page is a hair, ten thousand is a cord. */
export function widthOf(pages: number): number {
  return Math.min(10, 0.5 + 0.7 * Math.log2(Math.max(1, pages)))
}

/** How a node is painted: sources ink, hosts outlined, a refused host greyed. */
export function paintOf(node: GraphNode, palette: Palette): Pick<NodeAttributes, 'color' | 'borderColor'> {
  if (node.kind === 'source') return { color: palette.ink, borderColor: palette.ink }
  if (node.standing === 'ignored') return { color: palette.track, borderColor: palette.stone }
  return { color: palette.paper, borderColor: palette.ink }
}

/** "198,470 links on 11,442 pages", with both ends named. */
export function edgeTitle(edge: GraphEdge, nodes: Map<string, GraphNode>): { from: string; to: string; pages: number; links: number } {
  const name = (id: string) => { const node = nodes.get(id); return node ? labelOf(node) : id }
  return { from: name(edge.from), to: name(edge.to), pages: edge.pages, links: edge.links }
}

/** A small deterministic hash, so a host lands in the same place on every load. */
function hashOf(text: string): number {
  let h = 2166136261
  for (let i = 0; i < text.length; i++) { h ^= text.charCodeAt(i); h = Math.imul(h, 16777619) }
  return (h >>> 0) / 4294967296
}

/**
 * The server's view as a graphology graph with everything the renderer reads. Positions
 * come from `kept` when a node already had one — the layout survives a refetch — else
 * sources sit on a ring and each host starts beside the sources that link to it, which
 * gives the force layout a short way to go.
 */
export function toGraphology(view: GraphView, palette: Palette, kept: Map<string, { x: number; y: number }> = new Map()): Drawn {
  const graph = new Graph<NodeAttributes, EdgeAttributes>({ multi: false, type: 'directed' })
  const sources = view.nodes.filter(node => node.kind === 'source')
  const at = new Map<string, { x: number; y: number }>()
  sources.forEach((node, index) => {
    const angle = (index / Math.max(1, sources.length)) * 2 * Math.PI
    at.set(node.id, kept.get(node.id) ?? { x: Math.cos(angle), y: Math.sin(angle) })
  })
  const linkedFrom = new Map<string, string[]>()
  view.edges.forEach(edge => linkedFrom.set(edge.to, [...(linkedFrom.get(edge.to) ?? []), edge.from]))

  for (const node of view.nodes) {
    let position = kept.get(node.id) ?? at.get(node.id)
    if (!position) {
      const near = (linkedFrom.get(node.id) ?? []).map(id => at.get(id)).filter((p): p is { x: number; y: number } => Boolean(p))
      const centre = near.length
        ? { x: near.reduce((sum, p) => sum + p.x, 0) / near.length, y: near.reduce((sum, p) => sum + p.y, 0) / near.length }
        : { x: 0, y: 0 }
      const spin = hashOf(node.id) * 2 * Math.PI
      const reach = 0.2 + 0.5 * hashOf(`${node.id}:r`)
      position = { x: centre.x * 1.3 + Math.cos(spin) * reach, y: centre.y * 1.3 + Math.sin(spin) * reach }
    }
    graph.addNode(node.id, { ...position, ...paintOf(node, palette), size: sizeOf(node), label: labelOf(node), type: 'border', forceLabel: node.kind === 'source', node })
  }
  for (const edge of view.edges) {
    if (!graph.hasNode(edge.from) || !graph.hasNode(edge.to) || graph.hasEdge(edge.from, edge.to)) continue
    graph.addEdge(edge.from, edge.to, { size: widthOf(edge.pages), color: palette.rule, pages: edge.pages, links: edge.links })
  }
  return graph
}

/** Every position the graph holds, to carry across a refetch. */
export function positionsOf(graph: Drawn): Map<string, { x: number; y: number }> {
  const out = new Map<string, { x: number; y: number }>()
  graph.forEachNode((id, attributes) => out.set(id, { x: attributes.x, y: attributes.y }))
  return out
}

/**
 * Nodes matching a typed query: a name that starts with it first, then one that
 * contains it, each group busiest first. Sources before hosts within a group, since a
 * source is what the person most often means.
 */
export function searchNodes(nodes: GraphNode[], query: string, limit = 8): GraphNode[] {
  const needle = query.trim().toLowerCase()
  if (!needle) return []
  const weight = (node: GraphNode) => node.kind === 'source' ? Number.MAX_SAFE_INTEGER : node.pages
  const rank = (node: GraphNode) => { const name = labelOf(node).toLowerCase(); return name.startsWith(needle) ? 0 : name.includes(needle) ? 1 : -1 }
  return nodes
    .map(node => ({ node, rank: rank(node) }))
    .filter(entry => entry.rank >= 0)
    .sort((a, b) => a.rank - b.rank || weight(b.node) - weight(a.node) || labelOf(a.node).localeCompare(labelOf(b.node)))
    .slice(0, limit)
    .map(entry => entry.node)
}

/** A node and everything one edge away, for lighting its neighbourhood under the pointer. */
export function neighbourhood(graph: Drawn, id: string): Set<string> {
  const out = new Set<string>([id])
  if (graph.hasNode(id)) graph.forEachNeighbor(id, neighbour => out.add(neighbour))
  return out
}
