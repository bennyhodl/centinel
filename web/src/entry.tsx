import type { ReactNode } from 'react'
import crest from './assets/crest.jpg'

/** The crest, a title and a line under it: how every page before the workspace opens. */
export function EntryHeading({ title, children }: { title: string; children?: ReactNode }) {
  return <div className="flex flex-col items-center gap-2 text-center">
    <img src={crest} alt="" className="mb-1 size-12 rounded-md border border-foreground object-cover" />
    <h1 className="font-display text-2xl tracking-[0.08em]">{title}</h1>
    {children}
  </div>
}
