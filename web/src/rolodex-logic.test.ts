import { describe, expect, it } from 'vitest'

import type { RolodexShow } from './api'
import { plural } from './format'
import { howNamed, pageLabel, personGraph } from './rolodex-logic'

const viera: RolodexShow = {
  action: 'show',
  asked: 'luis viera',
  indexed: true,
  unread: 0,
  person: {
    name: 'Luis Viera',
    key: 'luis viera',
    spellings: { 'Luis Viera': 3 },
    pages: 3,
    sources: { tampa: { pages: 3, examples: ['https://www.tampa.gov/city-council'] } },
    emails: { 'lviera@tampagov.net': 2 },
    profiles: { 'x:luisvieratampa': { platform: 'x', handle: 'luisvieratampa', url: 'https://x.com/LuisVieraTampa', pages: 1 } },
    links: { 'https://www.tampa.gov/city-council/about-us/luis-viera': 3 },
  },
  sources: [
    { source: 'tampa', linked: 3, named: 38 },
    { source: 'cttv', linked: 0, named: 3 },
  ],
}

describe('a person as a graph', () => {
  it('puts the person at the centre, each source on an edge as heavy as it names them, and what their links point at beyond', () => {
    const { nodes, edges } = personGraph(viera)
    expect(nodes.map(node => node.id)).toEqual([
      'person:luis viera',
      'source:tampa',
      'source:cttv',
      'address:lviera@tampagov.net',
      'account:x:luisvieratampa',
      'page:https://www.tampa.gov/city-council/about-us/luis-viera',
    ])
    expect(edges).toContainEqual({ from: 'source:tampa', to: 'person:luis viera', pages: 41, links: 41 })
    expect(edges).toContainEqual({ from: 'source:cttv', to: 'person:luis viera', pages: 3, links: 3 })
    expect(edges).toContainEqual({ from: 'person:luis viera', to: 'account:x:luisvieratampa', pages: 1, links: 1 })
  })

  it('lays the star out itself: the person at the centre, sources above, what their links point at below', () => {
    const { positions = {} } = personGraph(viera)
    expect(positions['person:luis viera']).toEqual({ x: 0, y: 0 })
    expect([positions['source:tampa'], positions['source:cttv']]).toEqual([{ x: -1.2, y: 1 }, { x: 1.2, y: 1 }])
    expect(positions['address:lviera@tampagov.net']).toEqual({ x: -1.2, y: -1 })
    expect(positions['page:https://www.tampa.gov/city-council/about-us/luis-viera']).toEqual({ x: 1.2, y: -1 })
  })

  it('draws a name no link carries, with only the sources that write it', () => {
    const { nodes, edges } = personGraph({ action: 'show', asked: 'Jane Castor', indexed: true, unread: 0, sources: [{ source: 'tampa', linked: 0, named: 1 }] })
    expect(nodes).toEqual([
      { kind: 'person', id: 'person:jane castor', name: 'Jane Castor', pages: 0 },
      { kind: 'appearance', id: 'source:tampa', source: 'tampa', linked: 0, named: 1 },
    ])
    expect(edges).toEqual([{ from: 'source:tampa', to: 'person:jane castor', pages: 1, links: 1 }])
  })
})

describe('how a source names them', () => {
  it('says both halves, and leaves out the one that is zero', () => {
    expect(howNamed({ source: 'tampa', linked: 3, named: 38 }, plural)).toBe('linked from 3 pages · named in 38 documents')
    expect(howNamed({ source: 'cttv', linked: 0, named: 1 }, plural)).toBe('named in 1 document')
    expect(howNamed({ source: 'tampa', linked: 1 }, plural)).toBe('linked from 1 page')
  })

  it('shortens a page about them to its host and last segment', () => {
    expect(pageLabel('https://www.tampa.gov/city-council/about-us/luis-viera')).toBe('tampa.gov/…/luis-viera')
    expect(pageLabel('not a url')).toBe('not a url')
  })
})
