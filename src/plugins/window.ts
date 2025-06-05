import { invoke } from '@tauri-apps/api/core'
import { emit } from '@tauri-apps/api/event'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'

import { LISTEN_KEY } from '../constants'

type WindowLabel = 'main' | 'preference'

const COMMAND = {
  SHOW_WINDOW: 'plugin:custom-window|show_window',
  HIDE_WINDOW: 'plugin:custom-window|hide_window',
  SET_ALWAYS_ON_TOP: 'plugin:custom-window|set_always_on_top',
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

export function setAlwaysOnTop(alwaysOnTop: boolean) {
  invoke(COMMAND.SET_ALWAYS_ON_TOP, { alwaysOnTop })
}

export async function toggleWindowVisible(label?: WindowLabel) {
  const appWindow = getCurrentWebviewWindow()

  if (appWindow.label !== label) return

  const visible = await appWindow.isVisible()

  if (visible) {
    return hideWindow(label)
  }

  return showWindow(label)
}
