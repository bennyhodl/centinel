import { cp, readdir, readFile, rm, writeFile } from 'node:fs/promises'
import { promisify } from 'node:util'
import { brotliCompress, constants } from 'node:zlib'

// Start prerenders the SPA shell at build time. Cargo embeds web-dist/ as it stands:
// the shell at /web, its hashed client assets at /web/assets/.
await rm('web-dist', { recursive: true, force: true })
await cp('.start/client', 'web-dist', { recursive: true })

// The shell links the stylesheets the page gets; CSS imported from a component lands in
// a stylesheet nothing links, which only `vite dev` hides. Import CSS from styles.css.
const shell = await readFile('web-dist/index.html', 'utf8')
const unlinked = (await readdir('web-dist/assets')).filter(name => name.endsWith('.css') && !shell.includes(`/assets/${name}`))
if (unlinked.length) throw new Error(`the shell does not link ${unlinked.join(', ')}; import that CSS from web/src/styles.css`)

// Every script and stylesheet also ships compressed, beside itself as `<name>.br`. The
// work is done once here, at the highest quality, so serving one costs no CPU. The
// gateway hands a browser that takes brotli the `.br` file under the plain name.
const compress = promisify(brotliCompress)
for (const name of (await readdir('web-dist/assets')).filter(name => /\.(js|css)$/.test(name))) {
  const plain = await readFile(`web-dist/assets/${name}`)
  await writeFile(`web-dist/assets/${name}.br`, await compress(plain, { params: { [constants.BROTLI_PARAM_QUALITY]: constants.BROTLI_MAX_QUALITY, [constants.BROTLI_PARAM_SIZE_HINT]: plain.length } }))
}

// Ship the Start server with the Rust binary; Node builtins are the only externals.
const { build } = await import('esbuild')
await build({
  entryPoints: ['web/serve.mjs'], outfile: '.start/centinel-server.mjs', bundle: true,
  platform: 'node', format: 'esm', target: 'node22',
  banner: { js: "import { createRequire } from 'node:module'; const require = createRequire(import.meta.url);" },
  // ws asks for its optional native speed-ups by name; absent, it falls back to JavaScript.
  external: ['bun:sqlite', 'better-sqlite3', 'pg', 'mysql2', 'cloudflare:workers', 'bufferutil', 'utf-8-validate'],
})
