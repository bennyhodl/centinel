import Sigma, { DEFAULT_STYLES } from 'sigma'
import { layerBorder } from '@sigma/node-border'
import FA2Layout from 'graphology-layout-forceatlas2/worker'
import forceAtlas2 from 'graphology-layout-forceatlas2'
import { neighbourhood, positionsOf, toGraphology, type Drawn, type Hover, type Palette, type Shown } from './graph-data'

/**
 * The renderer over the page's graph, and the layout that settles it. In its own module,
 * loaded only in the browser: sigma reads `WebGL2RenderingContext` as it loads, and the
 * shell is rendered once in Node at build time.
 */
export type Canvas = {
  /** Draws a view, keeping the position of every node that was already on the canvas, then settles it. */
  show(view: Shown): void
  /** Brings one node to the centre and lights its neighbourhood. */
  focus(id: string): void
  kill(): void
}

export type Handlers = {
  hover(hover: Hover | null): void
  click(id: string): void
  /** While the force layout runs in its worker. */
  settling(on: boolean): void
}

/** The hover card is this wide, so near the right edge it flips to the pointer's left. */
const CARD_WIDTH = 340

export function mountGraph(container: HTMLElement, graph: Drawn, palette: Palette, on: Handlers): Canvas {
  let current: Hover | null = null
  const renderer = new Sigma(graph, container, {
    primitives: {
      nodes: {
        // The border layer reads `borderColor` from the styled node, not the graph, so the
        // fade below can recolour it.
        variables: { borderColor: { type: 'color', default: palette.ink } },
        layers: [layerBorder({ borders: [{ size: 0.18, mode: 'relative', color: { attribute: 'borderColor' } }, { size: 0, fill: true, color: { attribute: 'color' } }] })],
        label: { font: { family: 'Inter Variable, sans-serif', weight: '500', size: 12 } },
      },
    },
    // What the pointer lights, held as state rather than written into the graph.
    customNodeState: { isLit: false, isCurrent: false },
    customEdgeState: { isLit: false },
    customGraphState: { hasLit: false },
    styles: {
      nodes: [
        DEFAULT_STYLES.nodes,
        {
          borderColor: { attribute: 'borderColor' },
          labelColor: palette.ink,
          // The hover card is the page's; sigma draws only the label of the node it is about.
          labelVisibility: (node, state) => state.isHovered || state.isCurrent || node.forceLabel ? 'visible' : 'auto',
          backdropVisibility: { whenState: ['isHovered', 'isCurrent'], then: 'visible', else: 'hidden' },
          backdropColor: palette.paper,
        },
        // Under the pointer, a node and its neighbours keep their colour and the rest fade.
        { when: (_node, state, graphState) => graphState.hasLit && !state.isLit, then: { color: palette.track, borderColor: palette.track, labelVisibility: 'hidden' } },
        { whenState: 'isLit', then: { depth: 'topNodes' } },
      ],
      edges: [
        DEFAULT_STYLES.edges,
        { when: (_edge, _state, graphState) => graphState.hasLit, then: { color: palette.track } },
        { whenState: 'isLit', then: { color: palette.strong, depth: 'topEdges' } },
      ],
    },
    settings: {
      enableEdgeEvents: true,
      // Sizes are in pixels, as they were before sigma 4 made graph units the default.
      itemSizesReference: 'screen',
      labelRenderedSizeThreshold: 7,
      labelDensity: 0.4,
      minCameraRatio: 0.03,
      maxCameraRatio: 4,
    },
  })

  // The nodes and edges the last light set, so the next one can put them out.
  let litNodes: string[] = []
  let litEdges: string[] = []
  const light = (next: Hover | null) => {
    for (const node of litNodes) renderer.setNodeState(node, { isLit: false, isCurrent: false })
    for (const edge of litEdges) renderer.setEdgeState(edge, { isLit: false })
    current = next && { ...next, flip: next.x > renderer.getDimensions().width - CARD_WIDTH }
    litNodes = current ? [...current.lit] : []
    litEdges = !current ? [] : current.edge ? [current.edge] : current.node ? graph.edges(current.node) : []
    for (const node of litNodes) renderer.setNodeState(node, { isLit: true, isCurrent: node === current?.node })
    for (const edge of litEdges) renderer.setEdgeState(edge, { isLit: true })
    renderer.setGraphState({ hasLit: current !== null })
    on.hover(current)
    renderer.refresh({ skipIndexation: true })
  }
  renderer.on('enterNode', ({ node, event }) => light({ node, lit: neighbourhood(graph, node), x: event.x, y: event.y }))
  renderer.on('leaveNode', () => light(null))
  renderer.on('enterEdge', ({ edge, event }) => {
    const ends = graph.extremities(edge) as [string, string]
    light({ edge, ends, lit: new Set(ends), x: event.x, y: event.y })
  })
  renderer.on('leaveEdge', () => light(null))
  renderer.on('clickNode', ({ node }) => on.click(node))
  renderer.on('clickStage', () => light(null))
  renderer.on('downNode', () => light(null))

  // ForceAtlas2 in a worker, for a while that grows with the graph. It has no notion of
  // having converged, so it is stopped on a clock rather than left to run for ever.
  let layout: FA2Layout | null = null
  let timer: ReturnType<typeof setTimeout> | undefined
  const stop = () => {
    clearTimeout(timer)
    if (layout) { layout.kill(); layout = null; on.settling(false) }
  }
  const settle = () => {
    stop()
    if (!graph.order) return
    layout = new FA2Layout(graph, { settings: { ...forceAtlas2.inferSettings(graph), gravity: 1.2, slowDown: 4 }, getEdgeWeight: 'size' })
    layout.start()
    on.settling(true)
    timer = setTimeout(stop, Math.min(8_000, 1_500 + graph.order * 3))
  }

  return {
    show(view) {
      stop()
      const next = toGraphology(view, palette, positionsOf(graph))
      graph.clear()
      graph.import(next.export())
      if (!view.positions) settle()
    },
    focus(id) {
      const data = renderer.getNodeDisplayData(id)
      if (!data) return
      void renderer.getCamera().animate({ x: data.x, y: data.y, ratio: 0.25 }, { duration: 500 })
      const { width, height } = renderer.getDimensions()
      light({ node: id, lit: neighbourhood(graph, id), x: width / 2, y: height / 2 })
    },
    kill() {
      stop()
      renderer.kill()
    },
  }
}
