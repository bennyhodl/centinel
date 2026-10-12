import { SimplePool, useWebSocketImplementation } from 'nostr-tools/pool'
import { WebSocket } from 'ws'

// Node's own WebSocket and nostr-tools disagree on a failed connection: closing the socket
// from its error handler fires the error again, without end, and the exception that
// escapes would stop the gateway. The `ws` implementation closes quietly.
useWebSocketImplementation(WebSocket)

/** Profile metadata is optional display data. The signed public key remains identity. */
export async function nostrProfile(publicKey: string, relays: string[]): Promise<{ name?: string; image?: string }> {
  if (!relays.length) return {}
  const pool = new SimplePool()
  try {
    const events = await pool.querySync(relays, { kinds: [0], authors: [publicKey], limit: 1 }, { maxWait: 2500 })
    const latest = events.sort((a, b) => b.created_at - a.created_at)[0]
    if (!latest) return {}
    const metadata = JSON.parse(latest.content)
    const name = typeof metadata.display_name === 'string' ? metadata.display_name : metadata.name
    const image = typeof metadata.picture === 'string' && metadata.picture.startsWith('https://') ? metadata.picture : undefined
    return { name: typeof name === 'string' ? name.slice(0, 100) : undefined, image }
  } catch { return {} }
  finally { pool.close(relays) }
}
