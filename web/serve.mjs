import { serve } from '@hono/node-server'
import { Hono } from 'hono'
import { bodyLimit } from 'hono/body-limit'
import handler from '../.start/server/server.js'

const origin = process.env.CENTINEL_PUBLIC_URL
if (!origin || !process.env.CENTINEL_BACKEND || !process.env.CENTINEL_INTERNAL_SECRET || !process.env.CENTINEL_AUTH_ROOT) {
  throw new Error('Start this server with centinel serve or centinel web.')
}
// Initialize migrations before accepting traffic.
await handler.fetch(new Request(`${origin}/health`))
const app = new Hono()
app.use('*', bodyLimit({ maxSize: 2 * 1024 * 1024 }))
app.all('*', context => {
  const request = context.req.raw
  const url = new URL(request.url)
  const forwarded = new Request(`${origin}${url.pathname}${url.search}`, request)
  // Ignore client-supplied IP headers. A reverse proxy shares its rate-limit bucket.
  forwarded.headers.set('x-centinel-client-ip', context.env.incoming.socket.remoteAddress || 'unknown')
  return handler.fetch(forwarded)
})
const server = serve({ hostname: process.env.CENTINEL_HOST, port: Number(process.env.CENTINEL_PORT), fetch: app.fetch },
  () => process.stdout.write('centinel-start-ready\n'))
// Rust owns our lifetime. EOF also covers an ungraceful parent exit.
process.stdin.resume()
const shutdown = () => {
  // An open SSE response must not leave an orphan after Rust dies.
  setTimeout(() => process.exit(0), 5000).unref()
  server.close(async () => { await handler.close(); process.exit(0) })
}
process.stdin.on('end', shutdown)
for (const signal of ['SIGTERM', 'SIGINT']) process.on(signal, shutdown)
