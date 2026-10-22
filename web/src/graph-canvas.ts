import Sigma from 'sigma'
import { drawDiscNodeLabel } from 'sigma/rendering'
import { createNodeBorderProgram } from '@sigma/node-border'
import FA2Layout from 'graphology-layout-forceatlas2/worker'
import forceAtlas2 from 'graphology-layout-forceatlas2'
import { neighbourhood, positionsOf, toGraphology, type Drawn, type EdgeAttributes, type Hover, type NodeAttributes, type Palette, type Shown } from './graph-data'

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
  const renderer = new Sigma<NodeAttributes, EdgeAttributes>(graph, container, {
    defaultNodeType: 'border',
    nodeProgramClasses: {
      border: createNodeBorderProgram({ borders: [{ size: { value: 0.18, mode: 'relative' }, color: { attribute: 'borderColor' } }, { size: { fill: true }, color: { attribute: 'color' } }] }),
    },
    // The hover card is the page's; sigma draws only the label under the pointer.
    defaultDrawNodeHover: (context, data, settings) => drawDiscNodeLabel(context, data, settings),
    enableEdgeEvents: true,
    labelFont: 'Inter Variable, sans-serif',
    labelSize: 12,
    labelWeight: '500',
    labelColor: { color: palette.ink },
    labelRenderedSizeThreshold: 7,
    labelDensity: 0.4,
    zIndex: true,
    minCameraRatio: 0.03,
    maxCameraRatio: 4,
    // Under the pointer, a node and its neighbours keep their colour and the rest fade.
    nodeReducer: (node, data) => {
      if (!current) return data
      if (current.lit.has(node)) return { ...data, zIndex: 2, highlighted: node === current.node }
      return { ...data, color: palette.track, borderColor: palette.track, label: '', zIndex: 0 }
    },
    edgeReducer: (edge, data) => {
      if (!current) return data
      const lit = current.edge === edge || (current.node !== undefined && graph.extremities(edge).includes(current.node))
      return lit ? { ...data, color: palette.strong, zIndex: 1 } : { ...data, color: palette.track, zIndex: 0 }
    },
  })

  const light = (next: Hover | null) => {
    current = next && { ...next, flip: next.x > renderer.getDimensions().width - CARD_WIDTH }
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
