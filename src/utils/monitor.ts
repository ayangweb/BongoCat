import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { monitorFromPoint } from '@tauri-apps/api/window'
import { mapValues } from 'es-toolkit'

import { isMac } from './platform'

export interface CursorPoint {
  x: number
  y: number
}

export async function getCursorMonitor(point: CursorPoint) {
  let cursorPoint = point

  if (isMac) {
    const appWindow = getCurrentWebviewWindow()

    const scaleFactor = await appWindow.scaleFactor()

    cursorPoint = mapValues(cursorPoint, value => value * scaleFactor)
  }

  const { x, y } = point

  const monitor = await monitorFromPoint(x, y)

  if (!monitor) return

  return {
    ...monitor,
    cursorPoint,
  }
}
