import { useSyncExternalStore } from 'react'

export type Theme = 'light' | 'dark'

/**
 * Puts the colour mode on `<html>`: the one chosen here, else the system's. It names
 * nothing outside itself, because the shell also runs its source inline before the first
 * paint, so a dark page never flashes light.
 */
function applyTheme() {
  const chosen = localStorage.getItem('centinel-theme')
  const dark = chosen ? chosen === 'dark' : matchMedia('(prefers-color-scheme: dark)').matches
  document.documentElement.classList.toggle('dark', dark)
}

export const themeScript = `(${applyTheme})()`

const listeners = new Set<() => void>()
const changed = () => { applyTheme(); listeners.forEach(listener => listener()) }
if (typeof window !== 'undefined') matchMedia('(prefers-color-scheme: dark)').addEventListener('change', changed)

const subscribe = (listener: () => void) => { listeners.add(listener); return () => { listeners.delete(listener) } }
const current = (): Theme => document.documentElement.classList.contains('dark') ? 'dark' : 'light'

/** The colour mode in force, and a toggle that remembers the choice in this browser. */
export function useTheme() {
  const theme = useSyncExternalStore(subscribe, current, (): Theme => 'light')
  const toggle = () => { localStorage.setItem('centinel-theme', theme === 'dark' ? 'light' : 'dark'); changed() }
  return [theme, toggle] as const
}
