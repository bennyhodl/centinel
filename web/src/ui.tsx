import React from 'react'
import { Link } from '@tanstack/react-router'
import { CircleHelp } from 'lucide-react'
import type { DocumentIdentity } from './api'
import { decisionLabels, type Decision } from './policy'

export function PageHeader({ eyebrow, title, detail, children }: { eyebrow: string; title: string; detail?: string; children?: React.ReactNode }) {
  return <header className="page-header"><div><span className="eyebrow">{eyebrow}</span><h1>{title}</h1>{detail && <p>{detail}</p>}</div>{children}</header>
}
export function ErrorBox({ error }: { error: Error }) { return <div className="notice error" role="alert">{error.message}</div> }
export function Empty({ children }: { children: React.ReactNode }) { return <div className="empty"><CircleHelp /><p>{children}</p></div> }

export function DocumentLink({ doc, children, className }: { doc: DocumentIdentity; children: React.ReactNode; className?: string }) {
  return <Link className={className} to="/document/$sha" params={{ sha: doc.derived_sha }} search={{ source: doc.source, resource: doc.resource }} onClick={event => event.stopPropagation()}>{children}</Link>
}

/** A row of mutually exclusive buttons, for choices short enough to show all at once. */
export function Segmented<T extends string>({ value, options, onChange, label, small }: { value: T; options: Array<[T, string]>; onChange: (value: T) => void; label: string; small?: boolean }) {
  return <div className={`segmented ${small ? 'small' : ''}`} role="radiogroup" aria-label={label}>
    {options.map(([option, text]) => <button type="button" role="radio" aria-checked={option === value} className={option === value ? 'on' : ''} key={option} onClick={() => onChange(option)}>{text}</button>)}
  </div>
}

export function Spinner({ light }: { light?: boolean }) { return <span className={`spinner ${light ? 'light' : ''}`} aria-hidden /> }
export function Pulse() { return <span className="pulse" aria-hidden /> }

export function DecisionBadge({ decision }: { decision: Decision }) {
  return <span className={`decision ${decision}`}>{decisionLabels[decision]}</span>
}

/**
 * A probability on a 0–1 track with the threshold marked, and the review floor when
 * there is one, so a score reads against the policy that judged it.
 */
export function ScoreBar({ value, threshold, review, tone }: { value: number; threshold?: number; review?: number | null; tone?: string }) {
  const at = (x: number) => `${Math.max(0, Math.min(1, x)) * 100}%`
  return <span className={`scorebar ${tone || ''}`}>
    <span className="track">
      {review != null && threshold != null && <i className="band" style={{ left: at(review), width: at(threshold - review) }} />}
      <i className="fill" style={{ width: at(value) }} />
      {threshold != null && <b className="tick" style={{ left: at(threshold) }} />}
    </span>
    <span className="value">{value.toFixed(2)}</span>
  </span>
}
