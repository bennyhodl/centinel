import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { createFileRoute } from '@tanstack/react-router'
import { api } from '../api'
import { queries } from '../queries'
import { ErrorBox, PageHeader, SectionRule } from '../ui'
import { CopyBlock } from '../copy'

export const Route = createFileRoute('/connect')({
  loader: ({ context: { queryClient } }) => Promise.all([queryClient.ensureQueryData(queries.ops()), queryClient.ensureQueryData(queries.system())]),
  component: Connect,
})

/** How each client adds an MCP server that speaks HTTP. */
const clients = [
  { id: 'code', label: 'Claude Code', shell: true, body: (url: string) => `claude mcp add --transport http centinel ${url}`, note: 'Run once. Claude Code keeps it for every project.' },
  { id: 'desktop', label: 'Claude Desktop', shell: false, body: (url: string) => json({ command: 'npx', args: ['-y', 'mcp-remote', url] }), note: 'Add to claude_desktop_config.json, then restart Claude Desktop.' },
  { id: 'cursor', label: 'Cursor', shell: false, body: (url: string) => json({ url }), note: 'Add to ~/.cursor/mcp.json.' },
]

const json = (server: Record<string, unknown>) => JSON.stringify({ mcpServers: { centinel: server } }, null, 2)

function Connect() {
  const [client, setClient] = useState(clients[0].id)
  const system = useQuery(queries.system())
  const ops = useQuery(queries.ops())
  const tools = (ops.data?.ops || []).filter(op => op.mcp)
  const base = (system.data?.public_url || location.origin).replace(/\/+$/, '')
  const url = `${base}/mcp`
  const chosen = clients.find(c => c.id === client) || clients[0]
  return <>
    <PageHeader title="Connect an agent" detail="Give Claude, Cursor, or any MCP client this corpus. It can search, read and download. Collecting and classifying stay with you." />
    <div className="grid max-w-3xl gap-10">
      <section className="grid gap-4">
        <SectionRule aside={<span className="font-mono">{url}</span>}>1 · Add the server</SectionRule>
        <div className="inline-flex w-fit flex-wrap gap-0.5 rounded-lg bg-parchment p-1" role="tablist">
          {clients.map(c => <button type="button" role="tab" aria-selected={client === c.id} key={c.id} onClick={() => setClient(c.id)} className={`h-[30px] rounded-md px-3 text-[13px] ${client === c.id ? 'bg-background font-semibold shadow-xs' : 'text-muted-foreground hover:text-foreground'}`}>{c.label}</button>)}
        </div>
        <CopyBlock text={chosen.body(url)} shell={chosen.shell} />
        <p className="text-[13px] text-muted-foreground">{chosen.note} {system.data?.public_url ? 'The address comes from CENTINEL_PUBLIC_URL on the server.' : 'The address is this page’s host. Set CENTINEL_PUBLIC_URL on the server when agents reach it somewhere else.'}</p>
      </section>
      <section className="grid">
        <SectionRule aside={`${tools.length} tools · read only`}>2 · What the agent gets</SectionRule>
        {ops.error && <ErrorBox error={ops.error} />}
        {tools.map(op => <div key={op.name} className="flex gap-4 border-b py-2.5"><span className="w-28 shrink-0 font-mono text-[13px]">{op.name}</span><span className="text-sm text-[#3A352D]">{op.about}</span></div>)}
      </section>
    </div>
  </>
}
