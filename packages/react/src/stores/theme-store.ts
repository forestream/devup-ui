'use client'
import type { DevupTheme } from '../types/theme'

export type Theme = keyof DevupTheme | null
type StoreChangeEvent = (newTheme: Theme) => void

const LOCAL_STORAGE_KEY = '__DF_THEME_SELECTED__'
const initTheme =
  typeof localStorage !== 'undefined'
    ? ((localStorage.getItem(LOCAL_STORAGE_KEY) as Theme) ?? null)
    : null

export function createThemeStore() {
  if (typeof window === 'undefined')
    return {
      get: () => initTheme,
      set: () => {},
      subscribe: () => () => {},
    }

  const el = document.documentElement
  const subscribers: Set<StoreChangeEvent> = new Set()
  let theme: Theme = initTheme
  const get = () => theme
  const set = (newTheme: Theme) => {
    theme = newTheme
    document.documentElement.setAttribute('data-theme', newTheme ?? '')
    localStorage.setItem(LOCAL_STORAGE_KEY, newTheme ?? '')
    subscribers.forEach((subscriber) => subscriber(theme))
  }

  const subscribe = (onStoreChange: StoreChangeEvent) => {
    subscribers.add(onStoreChange)
    set(el.getAttribute('data-theme') as Theme)
    return () => subscribers.delete(onStoreChange)
  }

  return {
    get,
    set,
    subscribe,
  }
}
