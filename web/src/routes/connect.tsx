import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { createFileRoute } from '@tanstack/react-router'
import { api } from '../api'
import { ErrorBox, PageHeader, SectionRule } from '../ui'
import { CopyBlock } from '../copy'

export const Route = createFileRoute('/connect')({ component: Connect })

const desktopConfig = JSON.stringify({ mcpServers: { centinel: { command: 'centinel', args: ['mcp'] } } }, null, 2)

const clients = [
  { id: 'code', label: 'Claude Code', body: 'claude mcp add centinel -- centinel mcp', shell: true, note: 'Runs `centinel mcp` over stdio against this machine’s store.' },
  { id: 'desktop', label: 'Claude Desktop', body: desktopConfig, shell: false, note: 'Add to claude_desktop_config.json, then restart Claude Desktop.' },
  { id: 'cursor', label: 'Cursor', body: desktopConfig, shell: false, note: 'Add to ~/.cursor/mcp.json.' },
] as const

function Connect() {
  const [client, setClient] = useState<string>('code')
  const ops = useQuery({ queryKey: ['ops'], queryFn: api.ops, staleTime: Infinity })
  const tools = (ops.data?.ops || []).filter(op => op.mcp)
  const http = `${location.origin}/mcp`
  const chosen = clients.find(c => c.id === client)
  return <>
    <PageHeader title="Connect an agent" detail="Give Claude, Cursor, or any MCP client this corpus. It can search, read and download. Collecting and classifying stay with you." />
    <div className="grid max-w-3xl gap-10">
      <section className="grid gap-4">
        <SectionRule>1 · Add the server</SectionRule>
        <div className="inline-flex w-fit flex-wrap gap-0.5 rounded-lg bg-parchment p-1" role="tablist">
          {[...clients.map(c => [c.id, c.label]), ['http', 'Any client · HTTP']].map(([id, label]) => <button type="button" role="tab" aria-selected={client === id} key={id} onClick={() => setClient(id)} className={`h-[30px] rounded-md px-3 text-[13px] ${client === id ? 'bg-background font-semibold shadow-xs' : 'text-muted-foreground hover:text-foreground'}`}>{label}</button>)}
        </div>
        {chosen ? <CopyBlock text={chosen.body} shell={chosen.shell} /> : <CopyBlock text={http} shell={false} />}
        <p className="text-[13px] text-muted-foreground">{chosen ? chosen.note : 'Any MCP client that speaks HTTP. This server answers MCP at that address while it runs.'}</p>
      </section>
      <section className="grid">
        <SectionRule aside={`${tools.length} tools · read only`}>2 · What the agent gets</SectionRule>
        {ops.error && <ErrorBox error={ops.error} />}
        {tools.map(op => <div key={op.name} className="flex gap-4 border-b py-2.5"><span className="w-28 shrink-0 font-mono text-[13px]">{op.name}</span><span className="text-sm text-[#3A352D]">{op.about}</span></div>)}
      </section>
    </div>
  </>
}
