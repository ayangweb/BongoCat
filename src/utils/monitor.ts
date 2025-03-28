import {
  availableMonitors,
  cursorPosition,
} from '@tauri-apps/api/window'

export async function getCursorMonitor() {
  const monitors = await availableMonitors()

  if (!monitors.length) return

  const { x, y } = await cursorPosition()

  const monitor = monitors.find((monitor) => {
    const { position, size } = monitor

    const inX = x >= position.x && x <= position.x + size.width
    const inY = y >= position.y && y <= position.y + size.height

    return inX && inY
  })

  if (!monitor) return

  return {
    ...monitor,
    cursorX: x,
    cursorY: y,
  }
}
