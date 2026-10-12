import { createStartHandler, defaultStreamHandler } from '@tanstack/react-start/server'
import { createGateway } from './server/gateway'

const start = createStartHandler(defaultStreamHandler)
let gateway: ReturnType<typeof createGateway> | undefined
export default {
  async close() { (await gateway)?.close() },
  async fetch(request: Request) {
    // Build-time SPA prerendering needs no account store or backend.
    if (process.env.CENTINEL_BACKEND) {
      gateway ??= createGateway({
        root: process.env.CENTINEL_AUTH_ROOT!, origin: process.env.CENTINEL_PUBLIC_URL!,
        backend: process.env.CENTINEL_BACKEND, secret: process.env.CENTINEL_INTERNAL_SECRET!,
      })
      const response = await (await gateway).handle(request)
      if (response) return response
    }
    return start(request)
  },
}
