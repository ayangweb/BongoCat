import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import {
  cursorPosition,
  monitorFromPoint,
} from '@tauri-apps/api/window'

export async function getCursorMonitor() {
  const appWindow = getCurrentWebviewWindow()

  const scaleFactor = await appWindow.scaleFactor()

  const point = await cursorPosition()

  const { x, y } = point.toLogical(scaleFactor)

  const monitor = await monitorFromPoint(x, y)

  if (!monitor) return

  return {
    ...monitor,
    cursorPosition: point,
  }
}
