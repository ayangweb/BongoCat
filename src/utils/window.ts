import { PhysicalPosition } from '@tauri-apps/api/dpi'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'

import { getCursorMonitor } from './monitor'

import { useCatStore } from '@/stores/cat'

export async function adsorb() {
  const currentMonitor = await getCursorMonitor()

  if (!currentMonitor) return

  const catStore = useCatStore()

  const appWindow = getCurrentWebviewWindow()
  const windowPosition = await appWindow.outerPosition()
  const windowSize = await appWindow.innerSize()

  const { size: monitorSize, position: monitorPosition } = currentMonitor

  const windowOffsetX = windowPosition.x - monitorPosition.x
  const windowOffsetY = windowPosition.y - monitorPosition.y

  const adsorbThreshold = Math.min(monitorSize.width, monitorSize.height) * (catStore.adsorbRange / 100)

  let targetX = windowPosition.x
  let targetY = windowPosition.y

  const isNearLeft = windowOffsetX <= adsorbThreshold
  const isNearRight = (monitorSize.width - (windowOffsetX + windowSize.width)) <= adsorbThreshold
  const isNearTop = windowOffsetY <= adsorbThreshold
  const isNearBottom = (monitorSize.height - (windowOffsetY + windowSize.height)) <= adsorbThreshold

  if (isNearLeft) {
    targetX = monitorPosition.x
  }

  if (isNearRight) {
    targetX = monitorPosition.x + monitorSize.width - windowSize.width
  }

  if (isNearTop) {
    targetY = monitorPosition.y
  }

  if (isNearBottom) {
    targetY = monitorPosition.y + monitorSize.height - windowSize.height
  }

  return appWindow.setPosition(new PhysicalPosition(targetX, targetY))
}
