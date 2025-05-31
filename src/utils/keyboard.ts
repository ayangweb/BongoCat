import { upperFirst } from 'es-toolkit'

export function isModifierKey(key: string) {
  return ['control', 'shift', 'alt', 'meta'].includes(key)
}

export function normalizeKey(key: string) {
  const keyMap: Record<string, string> = {
    ' ': 'Space',
    'meta': 'Command',
  }

  if (keyMap[key]) return keyMap[key]

  return upperFirst(key)
}
