import { readFile, readdir, rm, mkdir, writeFile } from 'node:fs/promises'
import { join } from 'node:path'

// Start prerenders the SPA shell at build time. Embed its client assets as data
// URLs so Cargo and `centinel web --rebuild` can both serve a single HTML file.
const client = '.start/client'
let shell = await readFile(join(client, 'index.html'), 'utf8')
for (const name of await readdir(join(client, 'assets'))) {
  const mime = name.endsWith('.js') ? 'text/javascript' : name.endsWith('.css') ? 'text/css' : undefined
  if (!mime) throw new Error(`Unexpected client asset: ${name}`)
  const contents = await readFile(join(client, 'assets', name))
  shell = shell.replaceAll(`/web/assets/${name}`, `data:${mime};base64,${contents.toString('base64')}`)
}
if (shell.includes('/web/assets/')) throw new Error('SPA shell contains an unembedded asset')
await rm('web-dist', { recursive: true, force: true })
await mkdir('web-dist')
await writeFile('web-dist/index.html', shell)
