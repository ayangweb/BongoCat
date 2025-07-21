import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { cursorPosition, monitorFromPoint } from '@tauri-apps/api/window'

export async function getCursorMonitor() {
  const appWindow = getCurrentWebviewWindow()

  const scaleFactor = await appWindow.scaleFactor()

  const cursorPoint = await cursorPosition()

  const { x, y } = cursorPoint.toLogical(scaleFactor)

  const monitor = await monitorFromPoint(x, y)

  if (!monitor) return

  return {
    ...monitor,
    cursorPoint,
  }
}
