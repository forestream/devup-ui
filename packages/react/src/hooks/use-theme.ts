'use client'

import { useSyncExternalStore } from 'react'

import { createThemeStore, Theme } from '../stores/theme-store'

const themeStore = createThemeStore()

export function useTheme(): [Theme, (newTheme: Theme) => void] {
  const theme = useSyncExternalStore(
    themeStore.subscribe,
    themeStore.get,
    themeStore.get,
  )
  return [theme, themeStore.set]
}
