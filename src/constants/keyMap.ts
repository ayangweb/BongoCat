// @unocss-include
export const KEY_MAP = {
  // 1st row
  BackQuote: 'i-tabler-tilde',
  Numpad1: 'i-tabler-square-rounded-number-1',
  Numpad2: 'i-tabler-square-rounded-number-2',
  Numpad3: 'i-tabler-square-rounded-number-3',
  Numpad4: 'i-tabler-square-rounded-number-4',
  Numpad5: 'i-tabler-square-rounded-number-5',
  Numpad6: 'i-tabler-square-rounded-number-6',
  Numpad7: 'i-tabler-square-rounded-number-7',
  Numpad8: 'i-tabler-square-rounded-number-8',
  Numpad9: 'i-tabler-square-rounded-number-9',
  Numpad0: 'i-tabler-square-rounded-number-0',
  Minus: 'i-tabler-minus',
  Equal: 'i-tabler-equal',
  Backspace: 'i-tabler-backspace',

  // 2nd row
  Tab: 'i-tabler-arrow-big-right-line',
  KeyQ: 'i-tabler-square-rounded-letter-q',
  KeyW: 'i-tabler-square-rounded-letter-w',
  KeyE: 'i-tabler-square-rounded-letter-e',
  KeyR: 'i-tabler-square-rounded-letter-r',
  KeyT: 'i-tabler-square-rounded-letter-t',
  KeyY: 'i-tabler-square-rounded-letter-y',
  KeyU: 'i-tabler-square-rounded-letter-u',
  KeyI: 'i-tabler-square-rounded-letter-i',
  KeyO: 'i-tabler-square-rounded-letter-o',
  KeyP: 'i-tabler-square-rounded-letter-p',
  LeftBracket: 'i-tabler-brackets-contain-start',
  RightBracket: 'i-tabler-brackets-contain-end',
  BackSlash: 'i-tabler-slash',

  // 3rd row
  KeyA: 'i-tabler-square-rounded-letter-a',
  KeyS: 'i-tabler-square-rounded-letter-s',
  KeyD: 'i-tabler-square-rounded-letter-d',
  KeyF: 'i-tabler-square-rounded-letter-f',
  KeyG: 'i-tabler-square-rounded-letter-g',
  KeyH: 'i-tabler-square-rounded-letter-h',
  KeyJ: 'i-tabler-square-rounded-letter-j',
  KeyK: 'i-tabler-square-rounded-letter-k',
  KeyL: 'i-tabler-square-rounded-letter-l',
  SemiColon: 'i-tabler-semicolon',
  Quote: 'i-tabler-quote',

  // 4th row
  ShiftLeft: 'i-vaadin-shift',
  KeyZ: 'i-tabler-square-rounded-letter-z',
  KeyX: 'i-tabler-square-rounded-letter-x',
  KeyC: 'i-tabler-square-rounded-letter-c',
  KeyV: 'i-tabler-square-rounded-letter-v',
  KeyB: 'i-tabler-square-rounded-letter-b',
  KeyN: 'i-tabler-square-rounded-letter-n',
  KeyM: 'i-tabler-square-rounded-letter-m',
  Comma: 'i-tabler-comma',
  Dot: 'i-tabler-circle-dot',
  Slash: 'i-tabler-slash',
  ShiftRight: 'i-vaadin-shift',

  // 5th row
  CtrlLeft: 'i-vaadin-ctrl',
  Alt: 'i-ic-outline-keyboard-option-key',
  Space: 'i-tabler-space',
  AlyGr: 'i-vaadin-option',

  // function row
  UpArrow: 'i-tabler-square-rounded-arrow-up',
  LeftArrow: 'i-tabler-square-rounded-arrow-left',
  DownArrow: 'i-tabler-square-rounded-arrow-down',
  RightArrow: 'i-tabler-square-rounded-arrow-right',
} as const

// TODO 未匹配的
// capslock/ins/home/pgup/del/end/pgdn/numlock/scrolllock/enter/esc/f1-f12

export type KeyMap = keyof typeof KEY_MAP
