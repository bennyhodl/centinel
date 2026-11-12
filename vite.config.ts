import { defineConfig, type ProxyOptions } from 'vite'
import react, { reactCompilerPreset } from '@vitejs/plugin-react'
import babel from '@rolldown/plugin-babel'
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
    proxy.on('proxyReq', request => request.setHeader('Origin', process.env.CENTINEL_PUBLIC_URL || 'http://127.0.0.1:8787'))
  },
})

export default defineConfig({
  plugins: [
    tailwindcss(),
    tanstackStart({
      srcDirectory: 'web/src',
      // A page and its skeleton load as one chunk. Most skeletons are exported beside
      // their page, so splitting the page alone would keep every page in the entry.
      router: { basepath: '/web', codeSplittingOptions: { defaultBehavior: [['component', 'pendingComponent'], ['errorComponent'], ['notFoundComponent']] } },
      spa: { enabled: true, prerender: { outputPath: '/index.html' } },
    }),
    react(),
    // plugin-react 6 has no Babel of its own; the React Compiler is the one Babel pass left.
    babel({ presets: [reactCompilerPreset()] }),
  ],
  define: { __CENTINEL_VERSION__: JSON.stringify(version) },
  base: '/web/',
  resolve: {
    alias: { '@': fileURLToPath(new URL('./web/src', import.meta.url)) },
  },
  server: {
    proxy: {
      '/api': apiProxy(),
      '/.well-known': apiProxy(),
      '/workspace': apiProxy(),
      '/ops': apiProxy(),
      '/mcp': apiProxy(),
    },
  },
  // One stylesheet, linked by the shell, so no page waits on a second one. Scripts split
  // by route and fonts and images stay files: each is fetched once and kept by its hash.
  // An icon two pages share would otherwise be a request of its own; a shared chunk this
  // small is inlined into each chunk that imports it instead.
  environments: { client: { build: { cssCodeSplit: false, rolldownOptions: { output: { codeSplitting: { experimentalInlineCommonChunks: { maxSize: 10_000 } } } } } } },
  build: {
    outDir: '.start',
    emptyOutDir: true,
  },
})
