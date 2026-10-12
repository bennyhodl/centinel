import { SimplePool } from 'nostr-tools/pool'

const relays = ['wss://relay.damus.io', 'wss://nos.lol']
/** Profile metadata is optional display data. The signed public key remains identity. */
export async function nostrProfile(publicKey: string): Promise<{ name?: string; image?: string }> {
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
