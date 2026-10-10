import { useState } from 'react'

/** A command or a config, on ink, with a button that copies it exactly. */
export function CopyBlock({ text, shell }: { text: string; shell: boolean }) {
  const [copied, setCopied] = useState(false)
  const copy = () => navigator.clipboard.writeText(text).then(() => { setCopied(true); setTimeout(() => setCopied(false), 1500) })
  return <div className="flex items-start gap-3 rounded-[10px] bg-terminal px-4 py-3.5">
    {shell && <span className="font-mono text-sm leading-5 text-flame">$</span>}
    <pre className="min-w-0 flex-1 overflow-x-auto font-mono text-sm leading-5 text-terminal-ink">{text}</pre>
    <button type="button" onClick={copy} className="h-7 shrink-0 rounded-md px-2.5 text-xs font-semibold text-terminal-ink shadow-[inset_0_0_0_1px_#4A443A] hover:bg-white/5">{copied ? 'Copied' : 'Copy'}</button>
  </div>
}
