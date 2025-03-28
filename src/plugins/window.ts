import { invoke } from '@tauri-apps/api/core'
import { emit } from '@tauri-apps/api/event'

import { LISTEN_KEY } from '../constants'

type WindowLabel = 'main' | 'preference'

const COMMAND = {
  SHOW_WINDOW: 'plugin:custom-window|show_window',
  HIDE_WINDOW: 'plugin:custom-window|hide_window',
}

export function showWindow(label?: WindowLabel) {
  if (label) {
    emit(LISTEN_KEY.SHOW_WINDOW, label)
  } else {
    invoke(COMMAND.SHOW_WINDOW)
  }
}

export function hideWindow(label?: WindowLabel) {
  if (label) {
    emit(LISTEN_KEY.HIDE_WINDOW, label)
  } else {
    invoke(COMMAND.HIDE_WINDOW)
  }
}
