const fmt = new Intl.NumberFormat()

export const number = (value?: number | null, suffix = '') => value == null ? 'Unknown' : `${fmt.format(value)}${suffix}`
export const plural = (count: number, one: string, many = `${one}s`) => `${number(count)} ${count === 1 ? one : many}`
export const characters = (value?: number | null) => {
  if (value == null) return 'Unknown'
  if (value < 1_000) return `${value} chars`
  if (value < 1_000_000) return `${(value / 1_000).toFixed(1)}k chars`
  return `${(value / 1_000_000).toFixed(2)}m chars`
}
export const compact = (value: number) => value < 1_000 ? String(value) : value < 1_000_000 ? `${(value / 1_000).toFixed(1)}k` : `${(value / 1_000_000).toFixed(1)}M`
export const seconds = (ms?: number | null) => {
  if (ms == null || !Number.isFinite(ms)) return '—'
  if (ms < 1_000) return `${Math.max(0, Math.round(ms))} ms`
  if (ms < 60_000) return `${(ms / 1_000).toFixed(1)} s`
  const minutes = Math.floor(ms / 60_000)
  return `${minutes} min ${Math.round((ms % 60_000) / 1_000)} s`
}
/** Bytes, in the unit that keeps the number short. */
export const bytes = (n: number) => n < 1024 ? `${n} B` : n < 1024 * 1024 ? `${(n / 1024).toFixed(1)} KB` : `${(n / 1024 / 1024).toFixed(1)} MB`
/** A server timestamp to the minute, as the server wrote it: `2026-10-13 12:00`. */
export const when = (at: string) => at.slice(0, 16).replace('T', ' ')
/** The local time of day, to the second, of an instant in ms or an ISO string. */
export const clock = (at: number | string) => new Date(at).toTimeString().slice(0, 8)
export const money = (usd?: number | null) => usd == null ? 'Unknown' : usd < 0.01 ? `$${usd.toFixed(4)}` : `$${usd.toFixed(2)}`
export const probability = (value?: number | null) => value == null ? '—' : value.toFixed(2)

/** The last path segment of an address, which is usually the file or page name. */
export const tail = (url: string) => {
  try {
    const parsed = new URL(url)
    const parts = parsed.pathname.split('/').filter(Boolean)
    return decodeURIComponent(parts.at(-1) || parsed.hostname)
  } catch { return url }
}
