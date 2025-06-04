import { isMac } from './platform'

export interface Key {
  eventKey: string
  tauriKey?: string
  symbol?: string
}

export const modifierKeys: Key[] = [
  {
    eventKey: 'Shift',
    symbol: isMac ? '⇧' : 'Shift',
  },
  {
    eventKey: 'Control',
    symbol: isMac ? '⌃' : 'Ctrl',
  },
  {
    eventKey: 'Alt',
    symbol: isMac ? '⌥' : 'Alt',
  },
  {
    eventKey: 'Command',
    symbol: isMac ? '⌘' : 'Super',
  },
].map((item) => {
  return { ...item, tauriKey: item.eventKey }
})

export const standardKeys: Key[] = [
  // 第一排
  {
    eventKey: 'Escape',
    symbol: isMac ? '⎋' : 'Esc',
  },
  {
    eventKey: 'F1',
  },
  {
    eventKey: 'F2',
  },
  {
    eventKey: 'F3',
  },
  {
    eventKey: 'F4',
  },
  {
    eventKey: 'F5',
  },
  {
    eventKey: 'F6',
  },
  {
    eventKey: 'F7',
  },
  {
    eventKey: 'F8',
  },
  {
    eventKey: 'F9',
  },
  {
    eventKey: 'F10',
  },
  {
    eventKey: 'F11',
  },
  {
    eventKey: 'F12',
  }, // 第二排
  {
    eventKey: 'Backquote',
    symbol: '`',
  },
  {
    eventKey: 'Digit1',
  },
  {
    eventKey: 'Digit2',
  },
  {
    eventKey: 'Digit3',
  },
  {
    eventKey: 'Digit4',
  },
  {
    eventKey: 'Digit5',
  },
  {
    eventKey: 'Digit6',
  },
  {
    eventKey: 'Digit7',
  },
  {
    eventKey: 'Digit8',
  },
  {
    eventKey: 'Digit9',
  },
  {
    eventKey: 'Digit0',
  },
  {
    eventKey: 'Minus',
    tauriKey: '-',
    symbol: '-',
  },
  {
    eventKey: 'Equal',
    tauriKey: '=',
    symbol: '=',
  },
  {
    eventKey: 'Backspace',
    symbol: isMac ? '⌫' : void 0,
  },
  // 第三排
  {
    eventKey: 'Tab',
    symbol: isMac ? '⇥' : void 0,
  },
  {
    eventKey: 'KeyQ',
  },
  {
    eventKey: 'KeyW',
  },
  {
    eventKey: 'KeyE',
  },
  {
    eventKey: 'KeyR',
  },
  {
    eventKey: 'KeyT',
  },
  {
    eventKey: 'KeyY',
  },
  {
    eventKey: 'KeyU',
  },
  {
    eventKey: 'KeyI',
  },
  {
    eventKey: 'KeyO',
  },
  {
    eventKey: 'KeyP',
  },
  {
    eventKey: 'BracketLeft',
    symbol: '[',
  },
  {
    eventKey: 'BracketRight',
    symbol: ']',
  },
  {
    eventKey: 'Backslash',
    symbol: '\\',
  },
  // 第四排
  {
    eventKey: 'KeyA',
  },
  {
    eventKey: 'KeyS',
  },
  {
    eventKey: 'KeyD',
  },
  {
    eventKey: 'KeyF',
  },
  {
    eventKey: 'KeyG',
  },
  {
    eventKey: 'KeyH',
  },
  {
    eventKey: 'KeyJ',
  },
  {
    eventKey: 'KeyK',
  },
  {
    eventKey: 'KeyL',
  },
  {
    eventKey: 'Semicolon',
    symbol: ';',
  },
  {
    eventKey: 'Quote',
    symbol: '\'',
  },
  {
    eventKey: 'Enter',
    symbol: isMac ? '↩︎' : void 0,
  },
  // 第五排
  {
    eventKey: 'KeyZ',
  },
  {
    eventKey: 'KeyX',
  },
  {
    eventKey: 'KeyC',
  },
  {
    eventKey: 'KeyV',
  },
  {
    eventKey: 'KeyB',
  },
  {
    eventKey: 'KeyN',
  },
  {
    eventKey: 'KeyM',
  },
  {
    eventKey: 'Comma',
    symbol: ',',
  },
  {
    eventKey: 'Period',
    symbol: '.',
  },
  {
    eventKey: 'Slash',
    symbol: '/',
  },
  // 第六排
  {
    eventKey: 'Space',
    symbol: isMac ? '␣' : void 0,
  },
  // 方向键
  {
    eventKey: 'ArrowUp',
    symbol: '↑',
  },
  {
    eventKey: 'ArrowDown',
    symbol: '↓',
  },
  {
    eventKey: 'ArrowLeft',
    symbol: '←',
  },
  {
    eventKey: 'ArrowRight',
    symbol: '→',
  },
].map((item) => {
  const { eventKey } = item

  item.symbol ??= eventKey
  item.tauriKey ??= eventKey

  if (eventKey.startsWith('Digit') || eventKey.startsWith('Key')) {
    item.tauriKey = item.symbol = eventKey.slice(-1)
  }

  return item
})

export const keys = modifierKeys.concat(standardKeys)
