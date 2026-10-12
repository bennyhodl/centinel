import React, { useEffect } from 'react'
import { Link, useLocation, useParams } from '@tanstack/react-router'
import Markdown from 'react-markdown'
import rehypeSlug from 'rehype-slug'
import remarkGfm from 'remark-gfm'
import { assetUrl, pageAt, parts, resolveLink, sourceOf } from './book'

/** The documentation, read from the Markdown the web app bundles. */
export function Docs() {
  const slug = useParams({ strict: false })._splat ?? ''
  const hash = useLocation({ select: location => location.hash })
  const page = pageAt(slug)
  const source = page && sourceOf(page.path)

  // A new page starts at the top. An anchor scrolls to its heading once the page is drawn.
  useEffect(() => {
    const target = hash ? document.getElementById(hash.slice(1)) : null
    if (target) target.scrollIntoView()
    else window.scrollTo(0, 0)
  }, [slug, hash])

  return <div className="docs">
    <aside className="docs-nav" aria-label="Documentation pages">
      {parts.map((part, index) => <section key={index}>
        {part.title && <h2>{part.title}</h2>}
        <ul>{part.pages.map(item => <li key={item.slug}><Link to="/docs/$" params={{ _splat: item.slug }} activeOptions={{ exact: true }}>{item.title}</Link></li>)}</ul>
      </section>)}
    </aside>
    <article className="docs-article">
      {page && source
        ? <Markdown remarkPlugins={[remarkGfm]} rehypePlugins={[rehypeSlug]} components={{
            a: ({ href = '', children }) => <DocLink from={page.path} href={href}>{children}</DocLink>,
            img: ({ src = '', alt }) => <img src={assetUrl(page.path, src) ?? src} alt={alt} />,
          }}>{source}</Markdown>
        : <p className="docs-missing">No documentation page is called “{slug}”. <Link to="/docs/$" params={{ _splat: '' }}>Start at the beginning</Link>.</p>}
    </article>
  </div>
}

/** A link between book pages is a route, so the app's navigation carries it. Anything else is a plain link. */
function DocLink({ from, href, children }: { from: string; href: string; children: React.ReactNode }) {
  const target = resolveLink(from, href)
  if (!target) return <a href={href} target="_blank" rel="noreferrer">{children}</a>
  return <Link to="/docs/$" params={{ _splat: target.slug }} hash={target.hash.slice(1) || undefined}>{children}</Link>
}
