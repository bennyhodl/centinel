import { describe, expect, it } from 'vitest'

import type { GraphNode, GraphView } from './api'
import { edgeTitle, neighbourhood, paintOf, positionsOf, searchNodes, sizeOf, toGraphology, widthOf, type Palette } from './graph-data'

const palette: Palette = { ink: '#1A1712', paper: '#F4EEE1', stone: '#B9AE98', track: '#EFE9DC', rule: '#E2DACB', strong: '#3A352D' }

const source = (id: string, addresses: number): GraphNode => ({ kind: 'source', id: `source:${id}`, source: id, acquisition: 'site', target: `https://www.${id}.gov`, host: `www.${id}.gov`, enabled: true, addresses })
const host = (name: string, pages: number, standing: 'open' | 'ignored' = 'open'): GraphNode => ({ kind: 'host', id: `host:${name}`, host: name, pages, links: pages * 3, sources: 1, standing })

const view: GraphView = {
  nodes: [source('tampa', 19_330), source('hills', 2_000), host('www.tampaconnect.com', 11_442), host('www.facebook.com', 400, 'ignored'), host('publicrec.hillsclerk.com', 12)],
  edges: [
    { from: 'source:tampa', to: 'host:www.tampaconnect.com', pages: 11_442, links: 198_470 },
    { from: 'source:tampa', to: 'host:www.facebook.com', pages: 400, links: 400 },
    { from: 'source:tampa', to: 'host:publicrec.hillsclerk.com', pages: 10, links: 14 },
    { from: 'source:hills', to: 'host:publicrec.hillsclerk.com', pages: 2, links: 2 },
    { from: 'source:hills', to: 'source:tampa', pages: 7, links: 9 },
  ],
  totals: { sources: 2, hosts: 3, edges: 5, links: 198_895, open: 2, refused: 1 },
  truncated: false,
}

describe('weights', () => {
  it('widens an edge with the log of its pages, never past the cap', () => {
    expect(widthOf(1)).toBeCloseTo(0.5)
    expect(widthOf(2) - widthOf(1)).toBeCloseTo(widthOf(4) - widthOf(2))
    expect(widthOf(1_000_000)).toBe(10)
  })

  it('sizes a source by addresses and a host by pages, and a city does not dwarf a village', () => {
    expect(sizeOf(source('tampa', 19_330))).toBeGreaterThan(sizeOf(source('village', 50)))
    expect(sizeOf(source('tampa', 19_330)) / sizeOf(source('village', 50))).toBeLessThan(2)
    expect(sizeOf(host('a', 11_442))).toBeGreaterThan(sizeOf(host('b', 1)))
    expect(sizeOf(host('a', 11_442))).toBeLessThan(sizeOf(source('village', 50)))
  })

  it('paints sources filled, hosts outlined, refused hosts greyed', () => {
    expect(paintOf(source('tampa', 1), palette)).toEqual({ color: palette.ink, borderColor: palette.ink })
    expect(paintOf(host('x', 1), palette)).toEqual({ color: palette.paper, borderColor: palette.ink })
    expect(paintOf(host('x', 1, 'ignored'), palette)).toEqual({ color: palette.track, borderColor: palette.stone })
  })
})

describe('toGraphology', () => {
  it('carries every node and edge with both ends named', () => {
    const graph = toGraphology(view, palette)
    expect(graph.order).toBe(5)
    expect(graph.size).toBe(5)
    expect(graph.getEdgeAttributes('source:tampa', 'host:www.tampaconnect.com')).toMatchObject({ pages: 11_442, links: 198_470 })
    expect(graph.getNodeAttribute('source:tampa', 'forceLabel')).toBe(true)
    expect(graph.getNodeAttribute('host:www.facebook.com', 'forceLabel')).toBe(false)
    const nodes = new Map(view.nodes.map(node => [node.id, node]))
    expect(edgeTitle(view.edges[0], nodes)).toEqual({ from: 'tampa', to: 'www.tampaconnect.com', pages: 11_442, links: 198_470 })
    expect(edgeTitle(view.edges[4], nodes)).toMatchObject({ from: 'hills', to: 'tampa' })
  })

  it('keeps the positions it is given, so a refetch does not scatter the layout', () => {
    const first = toGraphology(view, palette)
    first.setNodeAttribute('host:www.facebook.com', 'x', 42)
    first.setNodeAttribute('host:www.facebook.com', 'y', -7)
    const again = toGraphology({ ...view, nodes: view.nodes.filter(node => node.id !== 'host:publicrec.hillsclerk.com') }, palette, positionsOf(first))
    expect(again.getNodeAttributes('host:www.facebook.com')).toMatchObject({ x: 42, y: -7 })
    expect(again.hasNode('host:publicrec.hillsclerk.com')).toBe(false)
  })

  it('starts a host beside the sources that link to it, and the same place every time', () => {
    const one = toGraphology(view, palette)
    const two = toGraphology(view, palette)
    expect(one.getNodeAttributes('host:publicrec.hillsclerk.com')).toEqual(two.getNodeAttributes('host:publicrec.hillsclerk.com'))
    const tampa = one.getNodeAttributes('source:tampa')
    const connect = one.getNodeAttributes('host:www.tampaconnect.com')
    const hills = one.getNodeAttributes('source:hills')
    const toTampa = Math.hypot(connect.x - tampa.x, connect.y - tampa.y)
    const toHills = Math.hypot(connect.x - hills.x, connect.y - hills.y)
    expect(toTampa).toBeLessThan(toHills)
  })

  it('lights a node and its neighbours', () => {
    const graph = toGraphology(view, palette)
    expect([...neighbourhood(graph, 'host:publicrec.hillsclerk.com')].sort()).toEqual(['host:publicrec.hillsclerk.com', 'source:hills', 'source:tampa'])
    expect([...neighbourhood(graph, 'host:nobody')]).toEqual(['host:nobody'])
  })
})

describe('searchNodes', () => {
  it('ranks a name that starts with the query over one that contains it, busiest first', () => {
    const found = searchNodes(view.nodes, 'hills').map(node => node.id)
    expect(found).toEqual(['source:hills', 'host:publicrec.hillsclerk.com'])
    expect(searchNodes(view.nodes, 'TAMPA').map(node => node.id)).toEqual(['source:tampa', 'host:www.tampaconnect.com'])
    expect(searchNodes(view.nodes, '  ')).toEqual([])
    expect(searchNodes(view.nodes, 'www', 1)).toHaveLength(1)
  })
})
