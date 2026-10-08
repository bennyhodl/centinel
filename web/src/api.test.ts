import { afterEach, describe, expect, it, vi } from 'vitest'

import { api } from './api'

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('API response validation', () => {
  it('rejects a non-JSON response', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('<!doctype html>', {
      headers: { 'content-type': 'text/html' },
    })))

    await expect(api.questions()).rejects.toThrow('The Centinel API is unavailable')
  })

  it('rejects JSON with an invalid shape', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({ questions: {} }), {
      headers: { 'content-type': 'application/json' },
    })))

    await expect(api.questions()).rejects.toThrow('/workspace/questions returned an invalid response')
  })
})
