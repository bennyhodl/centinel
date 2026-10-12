// The documentation is the Markdown in this folder. The web app bundles those files as
// text, so the page a reader sees is the file itself and nothing hosts a second copy.
// SUMMARY.md gives the order and the groups.

const sources = import.meta.glob<string>('./**/*.md', { query: '?raw', import: 'default', eager: true })
const files = new Map(Object.entries(sources).map(([path, text]) => [path.slice('./'.length), text]))
const images = import.meta.glob<string>('./assets/*', { import: 'default', eager: true })
const assets = new Map(Object.entries(images).map(([path, url]) => [path.slice('./'.length), url]))

export interface Page { title: string; path: string; slug: string }
export interface Part { title: string | null; pages: Page[] }

/** The URL segment for a page, such as `start/install`. The front page is the empty segment. */
function slugOf(path: string): string {
  return path === 'README.md' ? '' : path.replace(/\.md$/, '')
}

/**
 * The sections of SUMMARY.md. A `# heading` starts a titled section, and a `---` rule starts
 * an untitled one, the format mdBook's SUMMARY.md uses. The file's own `# Summary`
 * heading is its title, not a section.
 */
export function parseSummary(markdown: string): Part[] {
  const parts: Part[] = []
  let part: Part = { title: null, pages: [] }
  let titled = false
  for (const line of markdown.split('\n')) {
    const heading = /^# (.+)/.exec(line)
    if (heading && !titled) { titled = true; continue }
    if (heading || line.trim() === '---') {
      parts.push(part)
      part = { title: heading ? heading[1] : null, pages: [] }
      continue
    }
    const entry = /\[([^\]]+)\]\(([^)\s]+)\)/.exec(line)
    if (entry) part.pages.push({ title: entry[1], path: entry[2], slug: slugOf(entry[2]) })
  }
  parts.push(part)
  return parts.filter(section => section.pages.length > 0)
}

export const parts = parseSummary(files.get('SUMMARY.md') ?? '')
export const pages = parts.flatMap(part => part.pages)

export function pageAt(slug: string): Page | undefined {
  return pages.find(page => page.slug === slug)
}

export function sourceOf(path: string): string | undefined {
  return files.get(path)
}

const BASE = 'https://book.invalid/'

/**
 * Where a link inside a book page points, when it points at another page of the book. The
 * anchor is kept. Anything else (an outside URL, a file the book does not contain) is null,
 * and the caller leaves the link as written.
 */
export function resolveLink(from: string, href: string): { slug: string; hash: string } | null {
  const url = new URL(href, BASE + from)
  const path = url.pathname.slice(1)
  const page = pages.find(candidate => candidate.path === path)
  return page ? { slug: page.slug, hash: url.hash } : null
}

/** The bundled URL of an image a book page refers to, or undefined when the book does not carry it. */
export function assetUrl(from: string, src: string): string | undefined {
  return assets.get(new URL(src, BASE + from).pathname.slice(1))
}
