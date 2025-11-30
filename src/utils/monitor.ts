import type { PhysicalPosition } from '@tauri-apps/api/window'

import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { monitorFromPoint } from '@tauri-apps/api/window'

export async function getCursorMonitor(cursorPoint: PhysicalPosition) {
  const appWindow = getCurrentWebviewWindow()
  const scaleFactor = await appWindow.scaleFactor()

  const { x, y } = cursorPoint.toLogical(scaleFactor)

  const monitor = await monitorFromPoint(x, y)

  if (!monitor) return

  return monitor
}
