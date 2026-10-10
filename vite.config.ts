import { defineConfig, type ProxyOptions } from 'vite'
import react from '@vitejs/plugin-react'
import { tanstackStart } from '@tanstack/react-start/plugin/vite'
import tailwindcss from '@tailwindcss/vite'
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

const apiProxy = (): ProxyOptions => ({
  target: 'http://127.0.0.1:8787',
  changeOrigin: true,
  configure(proxy) {
    proxy.on('proxyReq', request => request.setHeader('Origin', 'http://127.0.0.1:8787'))
  },
})

export default defineConfig({
  plugins: [tailwindcss(), tanstackStart({ srcDirectory: 'web/src', router: { basepath: '/web' }, spa: { enabled: true, prerender: { outputPath: '/index.html' } } }), react({ babel: { plugins: ['babel-plugin-react-compiler'] } })],
  define: { __CENTINEL_VERSION__: JSON.stringify(version) },
  base: '/web/',
  resolve: {
    alias: { '@': fileURLToPath(new URL('./web/src', import.meta.url)) },
  },
  server: {
    proxy: {
      '/workspace': apiProxy(),
      '/ops': apiProxy(),
      '/mcp': apiProxy(),
    },
  },
  environments: { client: { build: { cssCodeSplit: false, rollupOptions: { output: { inlineDynamicImports: true } } } } },
  build: {
    outDir: '.start',
    emptyOutDir: true,
    assetsInlineLimit: 100_000_000
  },
})
