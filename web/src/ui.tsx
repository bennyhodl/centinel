import React from 'react'
import { Link } from '@tanstack/react-router'
import { CircleHelp } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Alert, AlertDescription } from '@/components/ui/alert'
import type { DocumentIdentity } from './api'
import { decisionLabels, type Decision } from './policy'

export function PageHeader({ eyebrow, title, detail, children }: { eyebrow: string; title: string; detail?: string; children?: React.ReactNode }) {
  return <header className="mb-6 flex flex-col justify-between gap-6 sm:flex-row sm:items-end [&_h1]:text-3xl [&_h1]:font-semibold [&_p]:mt-2 [&_p]:text-sm [&_p]:text-muted-foreground"><div><span className="text-xs font-medium text-muted-foreground">{eyebrow}</span><h1>{title}</h1>{detail && <p>{detail}</p>}</div>{children}</header>
}
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
