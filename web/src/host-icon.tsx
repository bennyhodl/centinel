import { cn } from 'cn'

/**
 * A host's icon — `tampa.gov`'s favicon — else its first letter. The one way the workspace
 * draws a host or a source, so a source carries the same mark on every page that names it.
 *
 * The letter is the fallback and the placeholder: it is drawn while the icon loads and
 * stays when the host has none.
 */
export function HostIcon({ host, size = 16, className }: { host: string; size?: number; className?: string }) {
  return <Monogram text={host.replace(/^www\./, '')} size={size} className={className} />
}

/** A source's icon: the icon of the host it collects. */
export function SourceIcon({ source, size = 16, className }: { source: string; size?: number; className?: string }) {
  return <Monogram text={source} size={size} className={className} />
}

function Monogram({ text, size, className }: { text: string; size: number; className?: string }) {
  return <span
    aria-hidden
    className={cn('inline-grid shrink-0 place-items-center rounded-[4px] bg-muted font-semibold text-muted-foreground', className)}
    style={{ width: size, height: size, fontSize: Math.round(size * 0.6) }}
  >{text.charAt(0).toUpperCase() || '?'}</span>
}
