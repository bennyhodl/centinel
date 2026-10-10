import { defineConfig } from 'vitest/config'
import { fileURLToPath, URL } from 'node:url'

/// The tests cover plain logic modules, not the app build, so they run without the
/// TanStack Start plugin. Start rewrites relative imports against its source directory,
/// and that breaks resolution for code outside a route.
export default defineConfig({
  root: 'web',
  resolve: {
    alias: { '@': fileURLToPath(new URL('./web/src', import.meta.url)) },
  },
  test: { include: ['src/**/*.test.ts'] },
})
