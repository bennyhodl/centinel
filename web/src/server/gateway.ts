import { nostrProfile } from './profile'
import { requireMcpAuth } from '@better-auth/mcp'
import { createIdentity } from './auth'
import type { Account } from '../api'

/** `relays` are asked for a Nostr user's name and picture at sign-in. None means never ask. */
export async function createGateway(config: { root: string; origin: string; backend: string; secret: string; relays: string[] }) {
  const identity = await createIdentity(config.root, config.origin)
  const { auth, db, member, session } = identity
  const json = (data: unknown, status = 200) => Response.json(data, { status, headers: { 'cache-control': 'no-store' } })
  const denied = () => json({ error: 'Administrator approval is required.' }, 403)
  async function forward(request: Request, user?: Account) {
    const url = new URL(request.url)
    const headers = new Headers()
    for (const name of ['content-type', 'accept', 'last-event-id', 'mcp-protocol-version']) {
      const value = request.headers.get(name)
      if (value) headers.set(name, value)
    }
    headers.set('x-centinel-secret', config.secret)
    headers.set('x-centinel-role', user?.role ?? 'user')
    headers.set('origin', config.backend)
    const response = await fetch(`${config.backend}${url.pathname}${url.search}`, {
      method: request.method, headers, signal: request.signal,
      body: ['GET', 'HEAD'].includes(request.method) ? undefined : await request.arrayBuffer(),
      redirect: 'manual',
    })
    const output = new Headers(response.headers)
    output.delete('content-encoding')
    output.delete('content-length')
    // Nothing forwarded is stored unless the backend says it may be: an icon is kept for a
    // week, and a signed-in user's list of documents is not.
    if (!output.has('cache-control')) output.set('cache-control', 'no-store')
    return new Response(response.body, { status: response.status, headers: output })
  }
  const mcpHandler = requireMcpAuth(auth, async (request, claims) => {
    const user = claims.sub ? member(claims.sub) : undefined
    if (!user || user.access !== 'approved') return denied()
    return forward(request, user)
  }, { resource: `${config.origin}/mcp`, requiredScopes: ['mcp:tools'] })

  async function handle(request: Request): Promise<Response | undefined> {
    const url = new URL(request.url)
    const path = url.pathname
    // Reject foreign browser origins before either cookie or token authentication.
    const origin = request.headers.get('origin')
    if (origin && origin !== config.origin) return json({ error: 'Foreign origin' }, 403)
    if (path === '/health') return json({ product: 'centinel', status: 'ok', public_url: config.origin, api_version: 1, version: process.env.CENTINEL_VERSION, store_id: process.env.CENTINEL_STORE_ID, build_id: process.env.CENTINEL_BUILD_ID })
    if (path === '/mcp') return mcpHandler(request)
    if (path.startsWith('/.well-known/')) return auth.handler(request)
    if (path.startsWith('/api/auth/')) {
      if (path === '/api/auth/oauth2/consent' || path === '/api/auth/oauth2/continue') {
        if ((await session(request))?.access !== 'approved') return denied()
      }
      const response = await auth.handler(request)
      if (path === '/api/auth/nostr/login' && response.ok) {
        const result = await response.clone().json()
        const id = result.user?.id
        const key = id && db.prepare('SELECT publicKey FROM nostrPubkey WHERE userId = ? LIMIT 1').get(id) as { publicKey: string } | undefined
        if (key) {
          const profile = await nostrProfile(key.publicKey, config.relays)
          db.prepare('UPDATE user SET name = COALESCE(?, name), image = COALESCE(?, image) WHERE id = ?').run(profile.name ?? null, profile.image ?? null, id)
        }
      }
      return response
    }
    if (path === '/api/account') {
      const user = await session(request)
      return json({ account: user ?? null })
    }
    if (path === '/api/admin/users') {
      const user = await session(request)
      if (user?.role !== 'admin' || user.access !== 'approved') return denied()
      if (request.method === 'GET') return json({ users: db.prepare(`SELECT u.id, u.name, u.email, u.image, u.role, u.access,
        (SELECT publicKey FROM nostrPubkey WHERE userId = u.id LIMIT 1) AS publicKey
        FROM user u ORDER BY u.createdAt DESC`).all() })
      if (request.method !== 'POST') return json({ error: 'Method not allowed' }, 405)
      if (origin !== config.origin) return json({ error: 'Same-origin request required' }, 403)
      const body = await request.json() as { id?: string; access?: string }
      if (!body.id || !['approved', 'revoked'].includes(body.access ?? '')) return json({ error: 'Invalid access change' }, 400)
      const target = member(body.id)
      if (!target) return json({ error: 'User not found' }, 404)
      if (target.role === 'admin') return json({ error: 'The administrator cannot be revoked.' }, 400)
      // Every request re-reads access, including OAuth tokens issued before revocation.
      db.prepare('UPDATE user SET access = ?, updatedAt = ? WHERE id = ?').run(body.access!, Date.now(), body.id)
      return json(member(body.id))
    }
    if (path.startsWith('/web/assets/')) return forward(request)
    if (path === '/ops' || path.startsWith('/ops/') || path.startsWith('/workspace/')) {
      const user = await session(request)
      if (!user) return json({ error: 'Sign in required' }, 401)
      if (user.access !== 'approved') return denied()
      if (!['GET', 'HEAD'].includes(request.method) && origin !== config.origin) return json({ error: 'Same-origin request required' }, 403)
      return forward(request, user)
    }
    if (path === '/') return Response.redirect(`${config.origin}/web`, 302)
    return undefined
  }
  return { handle, close: () => db.close() }
}
