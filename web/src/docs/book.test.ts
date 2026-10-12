import { describe, expect, it } from 'vitest'
import { assetUrl, pageAt, pages, parseSummary, parts, resolveLink, sourceOf } from './book'

describe('the book summary', () => {
  it('keeps the order and groups of SUMMARY.md', () => {
    expect(parts.map(part => part.title)).toEqual([null, 'Start', 'Use it', 'Operate it', 'How it works', 'Reference', null])
    expect(pageAt('start/install')?.title).toBe('Install')
  })

  it('names the front page with the empty slug', () => {
    expect(parseSummary('# Summary\n\n[Centinel](README.md)\n')).toEqual([{ title: null, pages: [{ title: 'Centinel', path: 'README.md', slug: '' }] }])
  })

  it('lists every page in the folder, so none is written and never shown', () => {
    const written = Object.keys(import.meta.glob('./**/*.md', { query: '?raw' })).map(path => path.slice('./'.length))
    const unlisted = written.filter(path => path !== 'SUMMARY.md' && !pages.some(page => page.path === path))
    expect(unlisted).toEqual([])
  })

  it('has source text for every page it lists, so no sidebar entry opens an empty page', () => {
    for (const page of pages) expect(sourceOf(page.path), page.path).toBeDefined()
  })
})

describe('links between pages', () => {
  it('resolves a relative link to the page it names and keeps the anchor', () => {
    expect(resolveLink('operate/doctor.md', '../internals/search.md#it-is-not-an-embedding-model'))
      .toEqual({ slug: 'internals/search', hash: '#it-is-not-an-embedding-model' })
  })

  it('resolves an anchor on the same page to that page', () => {
    expect(resolveLink('internals/search.md', '#refused-versus-truncated')).toEqual({ slug: 'internals/search', hash: '#refused-versus-truncated' })
  })

  it('resolves a link to the front page to the empty slug', () => {
    expect(resolveLink('start/install.md', '../README.md')).toEqual({ slug: '', hash: '' })
  })

  it('leaves links that go outside the book alone', () => {
    expect(resolveLink('start/install.md', 'https://github.com/bennyhodl/centinel/issues/7')).toBeNull()
  })

  it('finds every relative link in the book on a page of the book', () => {
    const broken = pages.flatMap(page => [...sourceOf(page.path)!.matchAll(/(!?)\[[^\]]*\]\(([^)\s]+)\)/g)]
      .filter(match => !/^[a-z][a-z0-9+.-]*:/i.test(match[2]))
      .filter(match => match[1] ? assetUrl(page.path, match[2]) === undefined : resolveLink(page.path, match[2]) === null)
      .map(match => `${page.path} -> ${match[2]}`))
    expect(broken).toEqual([])
  })

  it('bundles the image the front page shows', () => {
    expect(assetUrl('README.md', 'assets/centinel-watchman.jpg')).toBeDefined()
  })
})
