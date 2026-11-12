/**
 * Signing in and out. The gateway in front of the API runs Better Auth; this is its browser
 * side. Who is signed in is read through `queries.account()`, like everything else.
 *
 * The client loads on first use. Only a sign-in or a sign-out needs it and its Nostr
 * signing, so no page waits for either.
 */
const load = () => Promise.all([import('better-auth/react'), import('better-auth-nostr/client')])
  .then(([{ createAuthClient }, { nostrClient }]) => createAuthClient({ plugins: [nostrClient()] }))
let loading: ReturnType<typeof load> | undefined
const client = () => loading ??= load()

type Client = Awaited<ReturnType<typeof load>>
type Credentials = { email: string; password: string }

/** Better Auth answers `{ data, error }`. The pages want a thrown error or nothing. */
async function settle(call: (client: Client) => Promise<{ error: { message?: string } | null }>, fallback: string) {
  const { error } = await call(await client())
  if (error) throw new Error(error.message || fallback)
}

export const signIn = (credentials: Credentials) => settle(auth => auth.signIn.email(credentials), 'Sign-in failed')
export const signUp = (credentials: Credentials & { name: string }) => settle(auth => auth.signUp.email(credentials), 'The account could not be created')
/** Signs with the browser's Nostr extension. The key never leaves it. */
export const signInWithNostr = () => settle(auth => auth.signIn.nostr(), 'Nostr sign-in failed. Install a Nostr signer extension and try again.')

/** Ends the session and reloads from the sign-in page, so nothing read for this account stays cached. */
export async function signOut() {
  await (await client()).signOut()
  window.location.assign('/web/login')
}

/**
 * An MCP client's authorization request, carried in the query string Better Auth redirects
 * to the sign-in or consent page with. Null when the page was opened directly.
 */
export function oauthRequest(searchStr: string): URLSearchParams | null {
  const query = new URLSearchParams(searchStr)
  return query.has('client_id') ? query : null
}

/** After signing in: picks the authorization up where Better Auth left it. */
export const continueAuthorization = (request: URLSearchParams) => oauthStep('continue', { oauth_query: request.toString(), postLogin: true })
/** The person's answer to the client's request. */
export const answerConsent = (request: URLSearchParams, accept: boolean) => oauthStep('consent', { oauth_query: request.toString(), accept })

/** Either way, Better Auth says where the browser goes next, and it goes there. */
async function oauthStep(step: 'continue' | 'consent', body: Record<string, unknown>) {
  const response = await fetch(`/api/auth/oauth2/${step}`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(body) })
  const result = await response.json()
  if (!response.ok) throw new Error(result.message || result.error || 'Authorization failed')
  window.location.assign(result.url)
}
