import React from 'react'
import { Link } from '@tanstack/react-router'
import { CircleHelp } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Alert, AlertDescription } from '@/components/ui/alert'
import type { DocumentIdentity } from './api'
import { decisionLabels, type Decision } from './policy'

/**
 * The view-transition name a document's title carries, so the title glides from a search
 * result into the reader. Unique per Source, Resource and text, as a page needs.
 */
export function documentTransition(doc: DocumentIdentity): React.CSSProperties {
  let hash = 5381
  for (const ch of `${doc.source}\u0000${doc.resource}\u0000${doc.derived_sha}`) hash = (hash * 33 + ch.charCodeAt(0)) >>> 0
  return { viewTransitionName: `doc-${hash.toString(36)}` }
}

export function PageHeader({ eyebrow, title, detail, titleStyle, children }: { eyebrow?: React.ReactNode; title: string; detail?: string; titleStyle?: React.CSSProperties; children?: React.ReactNode }) {
  return <header className="mb-7 flex flex-col justify-between gap-6 sm:flex-row sm:items-end">
    <div className="min-w-0">
      {eyebrow && <div className="mb-2 text-[13px] text-muted-foreground">{eyebrow}</div>}
      <h1 className="w-fit font-serif text-[40px] leading-[44px] tracking-[-0.01em] wrap-anywhere" style={titleStyle}>{title}</h1>
      {detail && <p className="mt-2 text-sm text-muted-foreground">{detail}</p>}
    </div>
    {children}
  </header>
}

/** A ruled section label, the small caps line every list and panel starts with. */
export function SectionRule({ children, aside }: { children: React.ReactNode; aside?: React.ReactNode }) {
  return <div className="flex h-8 items-center justify-between border-b border-foreground">
    <span className="text-[11px] font-semibold uppercase tracking-[0.08em] text-muted-foreground">{children}</span>
    {aside && <span className="text-xs text-muted-foreground">{aside}</span>}
  </div>
}

/** The first seven characters of a hash: what Centinel prints, and takes back. */
export const shortSha = (sha: string) => sha.slice(0, 7)
export function ErrorBox({ error }: { error: Error }) { return <Alert variant="destructive" className="my-3"><AlertDescription>{error.message}</AlertDescription></Alert> }
export function Empty({ children }: { children: React.ReactNode }) { return <div className="grid place-items-center gap-3 p-12 text-center text-sm text-muted-foreground [&_svg]:size-6"><CircleHelp /><p>{children}</p></div> }

export function DocumentLink({ doc, children, className }: { doc: DocumentIdentity; children: React.ReactNode; className?: string }) {
  return <Link className={className} to="/document/$sha" params={{ sha: doc.derived_sha }} search={{ source: doc.source, resource: doc.resource }} onClick={event => event.stopPropagation()}>{children}</Link>
}

/** A row of mutually exclusive buttons, for choices short enough to show all at once. */
export function Segmented<T extends string>({ value, options, onChange, label, small }: { value: T; options: Array<[T, string]>; onChange: (value: T) => void; label: string; small?: boolean }) {
  return <div className={`inline-flex flex-wrap gap-1 rounded-md bg-muted p-1 ${small ? 'text-xs' : ''}`} role="radiogroup" aria-label={label}>
    {options.map(([option, text]) => <Button type="button" size={small ? 'sm' : 'default'} variant={option === value ? 'default' : 'ghost'} role="radio" aria-checked={option === value} key={option} onClick={() => onChange(option)}>{text}</Button>)}
  </div>
}

export function Spinner() { return <span className="inline-block size-3.5 shrink-0 animate-spin rounded-full border-2 border-current border-t-transparent motion-reduce:animate-none" aria-hidden /> }
export function Pulse() { return <span className="inline-block size-2 shrink-0 animate-pulse rounded-full bg-current motion-reduce:animate-none" aria-hidden /> }

export function DecisionBadge({ decision }: { decision: Decision }) {
  return <Badge variant={decision === 'error' ? 'destructive' : 'secondary'}>{decisionLabels[decision]}</Badge>
}

/**
 * A probability on a 0–1 track with the threshold marked, and the review floor when
 * there is one, so a score reads against the policy that judged it.
 */
export function ScoreBar({ value, threshold, review, tone }: { value: number; threshold?: number; review?: number | null; tone?: string }) {
  const at = (x: number) => `${Math.max(0, Math.min(1, x)) * 100}%`
  return <span className={`inline-flex w-full min-w-20 items-center gap-2 ${tone === 'error' ? 'text-destructive' : ''}`}>
    <span className="relative h-2 min-w-11 flex-1 rounded bg-muted">
      {review != null && threshold != null && <i className="absolute -inset-y-0.5 rounded bg-primary/20" style={{ left: at(review), width: at(threshold - review) }} />}
      <i className="absolute inset-y-0 left-0 rounded bg-primary/60" style={{ width: at(value) }} />
      {threshold != null && <b className="absolute -inset-y-1 w-0.5 rounded bg-primary" style={{ left: at(threshold) }} />}
    </span>
    <span className="min-w-8 text-right font-mono text-xs font-semibold">{value.toFixed(2)}</span>
  </span>
}
