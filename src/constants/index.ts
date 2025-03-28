export const LISTEN_KEY = {
  SHOW_WINDOW: 'show-window',
  HIDE_WINDOW: 'hide-window',
} as const

export const MODEL_PATH = {
  STANDARD: '/models/standard/cat.model3.json',
  KEYBOARD: '/models/keyboard/cat.model3.json',
} as const

export const MODEL_BACKGROUND = {
  STANDARD: '/images/backgrounds/standard.png',
  KEYBOARD: '/images/backgrounds/keyboard.png',
} as const

export type ModelType = keyof typeof MODEL_PATH
