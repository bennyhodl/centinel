import type { Appearance, RolodexShow } from './api'
import type { DrawnNode, Shown } from './graph-data'

/**
 * A person's report as the graph canvas draws it: the person at the centre, every source
 * the name comes up in around them, and the addresses, accounts and pages their links
 * point at. Pure, so the shape is tested without a canvas.
 */
export function personGraph(show: RolodexShow): Shown {
  const person = show.person
  const centre = `person:${person?.key ?? show.asked.toLowerCase()}`
  const nodes: DrawnNode[] = [{ kind: 'person', id: centre, name: person?.name ?? show.asked, pages: person?.pages ?? 0 }]
  const edges: Shown['edges'] = []

  for (const appearance of show.sources) {
    const id = `source:${appearance.source}`
    nodes.push({ kind: 'appearance', id, ...appearance })
    const pages = weightOf(appearance)
    edges.push({ from: id, to: centre, pages, links: pages })
  }

  const point = (what: 'address' | 'account' | 'page', key: string, label: string, url: string, pages: number) => {
    const id = `${what}:${key}`
    nodes.push({ kind: 'identifier', id, what, label, url, pages })
    edges.push({ from: centre, to: id, pages, links: pages })
  }
  for (const [email, pages] of Object.entries(person?.emails ?? {})) point('address', email, email, `mailto:${email}`, pages)
  for (const [key, profile] of Object.entries(person?.profiles ?? {})) point('account', key, `${profile.platform} · ${profile.handle}`, profile.url, profile.pages)
  for (const [url, pages] of Object.entries(person?.links ?? {})) point('page', url, pageLabel(url), url, pages)

  // The person at the centre, the sources in a row above and what their links point at
  // in a row below. Rows rather than a ring because every label runs to the right of its
  // node, and a wide, short graph keeps them inside the canvas.
  const positions: Shown['positions'] = { [centre]: { x: 0, y: 0 } }
  const row = (ids: string[], y: number) => ids.forEach((id, i) => {
    positions[id] = { x: ids.length === 1 ? 0 : -1.2 + (2.4 * i) / (ids.length - 1), y }
  })
  row(nodes.filter(node => node.kind === 'appearance').map(node => node.id), 1)
  row(nodes.filter(node => node.kind === 'identifier').map(node => node.id), -1)

  return { nodes, edges, positions }
}

/** How strongly a source names them: pages linking it and documents writing it. */
export const weightOf = (appearance: Appearance) => appearance.linked + (appearance.named ?? 0)

/** A page about them, as short as still says which: the host and the last segment. */
export function pageLabel(url: string): string {
  try {
    const parsed = new URL(url)
    const last = parsed.pathname.split('/').filter(Boolean).pop()
    return last ? `${parsed.host.replace(/^www\./, '')}/…/${last}` : parsed.host
  } catch {
    return url
  }
}

/** "linked from 3 pages · named in 38 documents", leaving out a half that is zero. */
export function howNamed(appearance: Appearance, plural: (count: number, one: string) => string): string {
  const parts: string[] = []
  if (appearance.linked) parts.push(`linked from ${plural(appearance.linked, 'page')}`)
  if (appearance.named) parts.push(`named in ${plural(appearance.named, 'document')}`)
  return parts.join(' · ')
}
