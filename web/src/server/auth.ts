import { DatabaseSync } from 'node:sqlite'
import { chmodSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { randomBytes } from 'node:crypto'
import { betterAuth, type BetterAuthOptions } from 'better-auth'
import { getMigrations } from 'better-auth/db/migration'
import { jwt } from 'better-auth/plugins'
import { mcp } from '@better-auth/mcp'
import { nostr } from 'better-auth-nostr'
import { cimd } from '@better-auth/cimd'
import { fetchClientMetadataResource } from '@better-auth/cimd/node'

import type { Account } from '../api'

export async function createIdentity(root: string, origin: string) {
  mkdirSync(root, { recursive: true, mode: 0o700 })
  const secretPath = join(root, 'secret')
  try { writeFileSync(secretPath, randomBytes(48).toString('hex'), { flag: 'wx', mode: 0o600 }) }
  catch (error) { if ((error as NodeJS.ErrnoException).code !== 'EEXIST') throw error }
  const databasePath = join(root, 'auth.sqlite')
  const db = new DatabaseSync(databasePath)
  chmodSync(databasePath, 0o600)
  db.exec('PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;')
  const options = {
    appName: 'Centinel',
    baseURL: origin,
    secret: readFileSync(secretPath, 'utf8'),
    database: db,
    trustedOrigins: [origin],
    advanced: { disableOriginCheck: false, ipAddress: { ipAddressHeaders: ['x-centinel-client-ip'] } },
    rateLimit: { enabled: true, storage: 'database', window: 60, max: 100 },
    emailAndPassword: { enabled: true, minPasswordLength: 12 },
    user: { additionalFields: {
      role: { type: 'string', required: true, defaultValue: 'user', input: false },
      access: { type: 'string', required: true, defaultValue: 'pending', input: false },
    } },
    session: { cookieCache: { enabled: false } },
    plugins: [nostr(), jwt(), cimd({ fetchClientMetadataResource, metadataProfile: 'mcp-2026-07-28' }), mcp({
      resource: `${origin}/mcp`, loginPage: '/web/login', consentPage: '/web/consent',
      scopes: ['openid', 'profile', 'offline_access', 'mcp:tools'],
      allowDynamicClientRegistration: true,
      allowUnauthenticatedClientRegistration: true,
      grantTypes: ['authorization_code', 'refresh_token'],
    })],
  } satisfies BetterAuthOptions
  await (await getMigrations(options)).runMigrations()
  // The trigger runs inside the user insert transaction, including concurrent signups.
  // The bootstrap record survives revocation/deletion; it can never elect another admin.
  db.exec(`
    CREATE TABLE IF NOT EXISTS centinel_bootstrap (id INTEGER PRIMARY KEY CHECK (id = 1), user_id TEXT NOT NULL);
    CREATE TRIGGER IF NOT EXISTS centinel_first_admin AFTER INSERT ON user
    WHEN NOT EXISTS (SELECT 1 FROM centinel_bootstrap)
    BEGIN
      INSERT INTO centinel_bootstrap VALUES (1, NEW.id);
      UPDATE user SET role = 'admin', access = 'approved' WHERE id = NEW.id;
    END;
  `)
  const auth = betterAuth(options)
  await (await auth.$context).checkSchema?.()
  function member(id: string): Account | undefined {
    return db.prepare('SELECT id, name, email, image, role, access FROM user WHERE id = ?').get(id) as Account | undefined
  }
  async function session(request: Request) {
    const result = await auth.api.getSession({ headers: request.headers })
    return result ? member(result.user.id) : undefined
  }
  return { auth, db, member, session }
}
