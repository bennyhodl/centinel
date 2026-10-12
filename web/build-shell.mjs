import { cp, readdir, readFile, rm } from 'node:fs/promises'

// Start prerenders the SPA shell at build time. Cargo embeds web-dist/ as it stands:
// the shell at /web, its hashed client assets at /web/assets/.
await rm('web-dist', { recursive: true, force: true })
await cp('.start/client', 'web-dist', { recursive: true })

// The shell links the stylesheets the page gets; CSS imported from a component lands in
// a stylesheet nothing links, which only `vite dev` hides. Import CSS from styles.css.
const shell = await readFile('web-dist/index.html', 'utf8')
const unlinked = (await readdir('web-dist/assets')).filter(name => name.endsWith('.css') && !shell.includes(`/assets/${name}`))
if (unlinked.length) throw new Error(`the shell does not link ${unlinked.join(', ')}; import that CSS from web/src/styles.css`)
