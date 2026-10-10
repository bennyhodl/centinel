import { cp, rm } from 'node:fs/promises'

// Start prerenders the SPA shell at build time. Cargo embeds web-dist/ as it stands:
// the shell at /web, its hashed client assets at /web/assets/.
await rm('web-dist', { recursive: true, force: true })
await cp('.start/client', 'web-dist', { recursive: true })
