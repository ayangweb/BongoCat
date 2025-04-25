import {
  availableMonitors,
  cursorPosition,
  primaryMonitor,
} from '@tauri-apps/api/window'

/**
 * 获取当前鼠标所在的显示器
 */
export async function getCursorMonitor() {
  const primary = await primaryMonitor()

  const monitors = await availableMonitors()

  if (!primary || !monitors.length) return

  const mousePosition = await cursorPosition()
  const { x, y } = mousePosition.toLogical(primary.scaleFactor)

  const monitor = monitors.find((monitor) => {
    const { scaleFactor } = monitor

    const position = monitor.position.toLogical(scaleFactor)
    const size = monitor.size.toLogical(scaleFactor)

    const inX = x >= position.x && x <= position.x + size.width
    const inY = y >= position.y && y <= position.y + size.height

    return inX && inY
  })

  if (!monitor) return

  const { scaleFactor, size, position } = monitor

  return {
    ...monitor,
    cursorX: x,
    cursorY: y,
    size: size.toLogical(scaleFactor),
    position: position.toLogical(scaleFactor),
  }
}
