import type { Monitor, PhysicalPosition } from '@tauri-apps/api/window'

import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { cursorPosition, monitorFromPoint } from '@tauri-apps/api/window'

function createCursorMonitor() {
  let cachedMonitor: Monitor | null = null

  return async (cursorPoint?: PhysicalPosition) => {
    cursorPoint ??= await cursorPosition()

    if (cachedMonitor) {
      const { size, position } = cachedMonitor

      const inBounds = cursorPoint.x >= position.x
        && cursorPoint.x < position.x + size.width
        && cursorPoint.y >= position.y
        && cursorPoint.y < position.y + size.height

      if (inBounds) {
        return cachedMonitor
      }
    }

    const appWindow = getCurrentWebviewWindow()

    const scaleFactor = await appWindow.scaleFactor()

    const { x, y } = cursorPoint.toLogical(scaleFactor)

    cachedMonitor = await monitorFromPoint(x, y)

    return cachedMonitor
  }
}

export const getCursorMonitor = createCursorMonitor()
