import { defineConfig, type ProxyOptions } from 'vite'
import react from '@vitejs/plugin-react'
import { viteSingleFile } from 'vite-plugin-singlefile'
import { fileURLToPath, URL } from 'node:url'
import { readFileSync } from 'node:fs'

/// The workspace carries the Centinel version it was built for. Cargo's build script
/// passes it in; a bare `npm run build` reads the same number from the workspace
/// Cargo.toml so the two never disagree.
function centinelVersion(): string {
  const fromCargo = process.env.CENTINEL_VERSION?.trim()
  if (fromCargo) return fromCargo
  const cargo = readFileSync(fileURLToPath(new URL('./Cargo.toml', import.meta.url)), 'utf8')
  const match = cargo.match(/\[workspace\.package\][^[]*?\nversion\s*=\s*"([^"]+)"/)
  if (!match) throw new Error('Cargo.toml has no [workspace.package] version')
  return match[1]
}

const version = centinelVersion()

const versionMeta = () => ({
  name: 'centinel-version-meta',
  transformIndexHtml: (html: string) => html.replace('<head>', `<head>\n    <meta name="centinel-version" content="${version}" />`),
})

const apiProxy = (): ProxyOptions => ({
  target: 'http://127.0.0.1:8787',
  changeOrigin: true,
  configure(proxy) {
    proxy.on('proxyReq', request => request.setHeader('Origin', 'http://127.0.0.1:8787'))
  },
})

export default defineConfig({
  plugins: [react(), viteSingleFile(), versionMeta()],
  define: { __CENTINEL_VERSION__: JSON.stringify(version) },
  root: 'web',
  base: '/web/',
  resolve: {
    alias: { '@': fileURLToPath(new URL('./web/src', import.meta.url)) },
  },
  server: {
    proxy: {
      '/workspace': apiProxy(),
      '/ops': apiProxy(),
    },
  },
  build: {
    outDir: '../web-dist',
    emptyOutDir: true,
    assetsInlineLimit: 100_000_000
  },
})
