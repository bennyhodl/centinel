import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { cn } from 'cn'
import { queries } from './queries'

/**
 * A host's icon — `tampa.gov`'s favicon — else its first letter. The one way the workspace
 * draws a host or a source, so a source carries the same mark on every page that names it.
 *
 * The letter is the fallback and the placeholder: it is drawn while the icon loads and
 * stays when the host has none. The icon is `/workspace/icons/{host}`, which the server
 * finds once and keeps; nothing else in the app knows how that happens. The mark is
 * decorative (the host is always named in text beside it) and the slot is `size` square
 * from the first paint, so an icon arriving or failing moves nothing.
 */
export function HostIcon({ host, size = 16, className }: { host: string; size?: number; className?: string }) {
  // The host the browser last heard back about, and what it said. Keyed by host so a slot
  // reused for another host starts again from the letter.
  const [heard, setHeard] = useState<{ host: string; found: boolean }>()
  const found = heard?.host === host ? heard.found : undefined
  return <span className={cn('relative inline-grid shrink-0', className)} style={{ width: size, height: size }}>
    <Monogram text={host.replace(/^www\./, '')} size={size} hidden={found === true} />
    {found !== false && <img
      aria-hidden alt="" loading="lazy" width={size} height={size}
      src={`/workspace/icons/${encodeURIComponent(host)}`}
      className={cn('absolute inset-0 rounded-[4px] object-contain', !found && 'opacity-0')}
      onLoad={() => setHeard({ host, found: true })}
      onError={() => setHeard({ host, found: false })}
    />}
  </span>
}

/**
 * A source's icon: the icon of the host it collects. A source is named by its id on every
 * page, so the host comes from one small read any signed-in user may make; until it
 * arrives, or for a source it does not list, the source's own letter stands in.
 */
export function SourceIcon({ source, size = 16, className }: { source: string; size?: number; className?: string }) {
  const host = useQuery(queries.sourceHosts()).data?.[source]
  return host
    ? <HostIcon host={host} size={size} className={className} />
    : <span className={cn('inline-grid shrink-0', className)}><Monogram text={source} size={size} /></span>
}

function Monogram({ text, size, hidden }: { text: string; size: number; hidden?: boolean }) {
  return <span
    aria-hidden
    className={cn('inline-grid shrink-0 place-items-center rounded-[4px] bg-muted font-semibold text-muted-foreground', hidden && 'invisible')}
    style={{ width: size, height: size, fontSize: Math.round(size * 0.6) }}
  >{text.charAt(0).toUpperCase() || '?'}</span>
}
