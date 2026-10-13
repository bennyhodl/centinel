import { afterAll, beforeAll, expect, test } from 'vitest'
import { mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { get } from 'node:http'
import { finalizeEvent, generateSecretKey, getPublicKey } from 'nostr-tools/pure'
import { createIdentity } from './auth'
import { createHash, randomBytes } from 'node:crypto'
import { serve } from '@hono/node-server'
import { createGateway } from './gateway'

const root = mkdtempSync(join(tmpdir(), 'centinel-auth-'))
let gateway: Awaited<ReturnType<typeof createGateway>>
let origin: string
let server: ReturnType<typeof serve>
let backend: ReturnType<typeof serve>
let forwarded = 0
beforeAll(async () => {
  backend = serve({ hostname: '127.0.0.1', port: 0, fetch(request) {
    expect(request.headers.get('x-centinel-secret')).toBe('private-test-credential')
    expect(request.headers.get('x-centinel-role')).toBe('user')
    forwarded++
    return Response.json({ result: 'corpus result' })
  } })
  server = serve({ hostname: '127.0.0.1', port: 0, fetch: async request => await gateway.handle(request) ?? new Response('page') })
  await Promise.all([server, backend].map(s => new Promise<void>(resolve => s.listening ? resolve() : s.once('listening', resolve))))
  origin = `http://127.0.0.1:${(server.address() as { port: number }).port}`
  gateway = await createGateway({ root, origin, backend: `http://127.0.0.1:${(backend.address() as { port: number }).port}`, secret: 'private-test-credential', relays: [] })
}, 30_000)
afterAll(async () => {
  await Promise.all([server, backend].filter(Boolean).map(s => new Promise<void>(resolve => s.close(() => resolve()))))
  gateway?.close()
  rmSync(root, { recursive: true, force: true })
})
function request(path: string, body?: unknown, cookie?: string, extra: Record<string, string> = {}) {
  return fetch(origin + path, { method: body === undefined ? 'GET' : 'POST', redirect: 'manual', headers: {
    origin, 'content-type': 'application/json', ...(cookie ? { cookie } : {}), ...extra,
  }, body: body === undefined ? undefined : JSON.stringify(body) })
}
async function signup(email: string) {
  const response = await request('/api/auth/sign-up/email', { email, name: email.split('@')[0], password: 'twelve-or-more-characters', role: 'admin', access: 'approved' })
  expect(response.status).toBe(200)
  const cookie = response.headers.getSetCookie().map(c => c.split(';')[0]).join('; ')
  const { account } = await (await request('/api/account', undefined, cookie)).json()
  return { cookie, account }
}

test('signup approval, OAuth MCP access, and revocation share current account permissions', async () => {
  expect((await request('/workspace/documents')).status).toBe(401)
  const challenge = await request('/mcp', { jsonrpc: '2.0', id: 1, method: 'initialize' })
  expect(challenge.status).toBe(401)
  expect(challenge.headers.get('www-authenticate')).toContain('oauth-protected-resource')
  const protectedMetadata = await request('/.well-known/oauth-protected-resource/mcp')
  expect(protectedMetadata.status).toBe(200)
  expect((await protectedMetadata.json()).resource).toBe(`${origin}/mcp`)
  const bootstrap = await Promise.all([signup('operator@example.com'), signup('concurrent@example.com')])
  expect(bootstrap.filter(user => user.account.role === 'admin')).toHaveLength(1)
  const admin = bootstrap.find(user => user.account.role === 'admin')!
  expect(admin.account).toMatchObject({ role: 'admin', access: 'approved' })
  const first = await signup('reader@example.com')
  const second = bootstrap.find(user => user.account.role !== 'admin')!
  for (const user of [first, second]) expect(user.account).toMatchObject({ role: 'user', access: 'pending' })
  expect((await request('/workspace/documents', undefined, first.cookie)).status).toBe(403)
  expect((await request('/api/admin/users', { id: first.account.id, access: 'approved' }, first.cookie)).status).toBe(403)
  expect((await request('/api/admin/users', { id: admin.account.id, access: 'revoked' }, admin.cookie)).status).toBe(400)
  expect((await request('/api/admin/users', { id: first.account.id, access: 'approved' }, admin.cookie, { origin: 'https://foreign.example' })).status).toBe(403)
  expect((await request('/api/admin/users', { id: first.account.id, access: 'approved' }, admin.cookie)).status).toBe(200)
  expect((await request('/workspace/documents', undefined, first.cookie, { 'x-centinel-role': 'admin' })).status).toBe(200)
  expect(forwarded).toBe(1)

  const discoveryResponse = await request('/.well-known/oauth-authorization-server/api/auth')
  expect(discoveryResponse.status).toBe(200)
  const metadata = await discoveryResponse.json()
  const registration = await request('/api/auth/oauth2/register', {
    client_name: 'Integration test', application_type: 'native', redirect_uris: ['http://127.0.0.1:19451/callback'],
    token_endpoint_auth_method: 'none', grant_types: ['authorization_code', 'refresh_token'],
    response_types: ['code'], scope: 'openid mcp:tools offline_access',
  })
  expect(registration.status, await registration.clone().text()).toBe(201)
  const client = await registration.json()
  const verifier = randomBytes(32).toString('base64url')
  const query = new URLSearchParams({ client_id: client.client_id, redirect_uri: 'http://127.0.0.1:19451/callback',
    response_type: 'code', scope: 'openid mcp:tools offline_access', state: 'test-state',
    code_challenge_method: 'S256', code_challenge: createHash('sha256').update(verifier).digest('base64url'), resource: `${origin}/mcp` })
  const authorization = await new Promise<Response>((resolve, reject) => {
    get(`${origin}/api/auth/oauth2/authorize?${query}`, { headers: { cookie: first.cookie, accept: 'text/html', 'sec-fetch-mode': 'navigate' } }, response => {
      response.resume()
      resolve(new Response(null, { status: response.statusCode, headers: { location: response.headers.location! } }))
    }).on('error', reject)
  })
  expect(authorization.status).toBe(302)
  const consentUrl = new URL(authorization.headers.get('location')!, origin)
  expect(consentUrl.pathname).toBe('/web/consent')
  const consent = await request('/api/auth/oauth2/consent', { accept: true, oauth_query: consentUrl.search.slice(1) }, first.cookie)
  expect(consent.status).toBe(200)
  const callback = new URL((await consent.json()).url)
  expect(callback.searchParams.get('state')).toBe('test-state')
  const tokenResponse = await fetch(metadata.token_endpoint, { method: 'POST', headers: { 'content-type': 'application/x-www-form-urlencoded' }, body: new URLSearchParams({ grant_type: 'authorization_code', code: callback.searchParams.get('code')!, client_id: client.client_id, redirect_uri: 'http://127.0.0.1:19451/callback', code_verifier: verifier, resource: `${origin}/mcp` }) })
  expect(tokenResponse.status).toBe(200)
  const token = await tokenResponse.json()
  expect((await request('/mcp', { jsonrpc: '2.0', id: 1, method: 'tools/list' }, undefined, { authorization: `Bearer ${token.access_token}` })).status).toBe(200)
  // An agent's token is not a browser session. It never reaches /workspace, where an
  // administrator can start collection — only the browser carries that authority.
  expect((await request('/workspace/sources', undefined, undefined, { authorization: `Bearer ${token.access_token}` })).status).toBe(401)
  expect((await request('/workspace/sources/try', { url: 'https://example.gov/' }, undefined, { authorization: `Bearer ${token.access_token}` })).status).toBe(401)
  expect(forwarded).toBe(2)
  expect((await request('/api/admin/users', { id: first.account.id, access: 'revoked' }, admin.cookie)).status).toBe(200)
  expect((await request('/workspace/documents', undefined, first.cookie)).status).toBe(403)
  expect((await request('/mcp', { jsonrpc: '2.0', id: 2, method: 'tools/list' }, undefined, { authorization: `Bearer ${token.access_token}` })).status).toBe(403)
  expect(forwarded).toBe(2)
  const { users } = await (await request('/api/admin/users', undefined, admin.cookie)).json()
  expect(users.find((u: { id: string }) => u.id === first.account.id).access).toBe('revoked')
  const reopened = await createIdentity(root, origin)
  expect(reopened.member(first.account.id)?.access).toBe('revoked')
  expect(reopened.member(admin.account.id)?.role).toBe('admin')
  reopened.db.close()

  // A real NIP-98 signature creates a pending user. Reusing its nonce must fail.
  const secretKey = generateSecretKey()
  const publicKey = getPublicKey(secretKey)
  const nonce = await (await request('/api/auth/nostr/nonce', { publicKey })).json()
  const body = { nonce: nonce.nonce }
  const event = finalizeEvent({ kind: 27235, created_at: Math.floor(Date.now() / 1000), content: '',
    tags: [['u', `${origin}/api/auth/nostr/login`], ['method', 'POST'], ['payload', createHash('sha256').update(JSON.stringify(body)).digest('hex')]],
  }, secretKey)
  const authorizationHeader = `Nostr ${Buffer.from(JSON.stringify(event)).toString('base64')}`
  const nostr = await request('/api/auth/nostr/login', body, undefined, { authorization: authorizationHeader })
  expect(nostr.status, await nostr.clone().text()).toBe(200)
  const nostrCookie = nostr.headers.getSetCookie().map(c => c.split(';')[0]).join('; ')
  expect((await (await request('/api/account', undefined, nostrCookie)).json()).account).toMatchObject({ role: 'user', access: 'pending' })
  expect((await request('/api/auth/nostr/login', body, undefined, { authorization: authorizationHeader })).status).toBe(401)
}, 30_000)
