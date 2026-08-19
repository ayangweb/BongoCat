import type { Event } from '@tauri-apps/api/event'

import { PhysicalPosition, PhysicalSize } from '@tauri-apps/api/dpi'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { availableMonitors } from '@tauri-apps/api/window'
import { useDebounceFn } from '@vueuse/core'
import { isNumber } from 'es-toolkit/compat'
import { onMounted, ref, watch } from 'vue'

import { WINDOW_LABEL } from '@/constants'
import { useAppStore } from '@/stores/app'
import { useCatStore } from '@/stores/cat'
import { getCursorMonitor } from '@/utils/monitor'

export type WindowState = Record<string, Partial<PhysicalPosition & PhysicalSize> | undefined>

const appWindow = getCurrentWebviewWindow()
const { label } = appWindow

export function useWindowState() {
  const appStore = useAppStore()
  const catStore = useCatStore()
  const isRestored = ref(false)

  onMounted(() => {
    appWindow.onMoved(onChange)

    appWindow.onResized(onChange)

    appWindow.onScaleChanged(clampToMonitor)
  })

  const clampToMonitor = useDebounceFn(async () => {
    if (label !== WINDOW_LABEL.MAIN || !catStore.window.keepInScreen) return

    const monitor = await getCursorMonitor()

    if (!monitor) return

    const { position: monitorPos, size: monitorSize } = monitor
    const windowSize = await appWindow.outerSize()
    const windowPos = await appWindow.outerPosition()

    const minX = monitorPos.x
    const maxX = monitorPos.x + monitorSize.width - windowSize.width
    const minY = monitorPos.y
    const maxY = monitorPos.y + monitorSize.height - windowSize.height

    const clampedX = Math.max(minX, Math.min(windowPos.x, maxX))
    const clampedY = Math.max(minY, Math.min(windowPos.y, maxY))

    if (clampedX === windowPos.x && clampedY === windowPos.y) return

    return appWindow.setPosition(new PhysicalPosition(clampedX, clampedY))
  }, 500)

  watch(() => catStore.window.keepInScreen, clampToMonitor)

  const onChange = async (event: Event<PhysicalPosition | PhysicalSize>) => {
    const minimized = await appWindow.isMinimized()

    if (minimized) return

    if ('width' in event.payload && 'height' in event.payload
      && (event.payload.width <= 0 || event.payload.height <= 0)) {
      return
    }

    appStore.windowState[label] ??= {}

    Object.assign(appStore.windowState[label], event.payload)

    clampToMonitor()
  }

  const restoreState = async () => {
    const { x, y, width, height } = appStore.windowState[label] ?? {}

    if (isNumber(x) && isNumber(y)) {
      const monitors = await availableMonitors()

      const monitor = monitors.find((monitor) => {
        const { position, size } = monitor

        const inBoundsX = x >= position.x && x <= position.x + size.width
        const inBoundsY = y >= position.y && y <= position.y + size.height

        return inBoundsX && inBoundsY
      })

      if (monitor) {
        await appWindow.setPosition(new PhysicalPosition(x, y))
      }
    }

    if (isNumber(width) && width > 0 && isNumber(height) && height > 0) {
      await appWindow.setSize(new PhysicalSize(width, height))
    } else if (appStore.windowState[label]) {
      delete appStore.windowState[label].width
      delete appStore.windowState[label].height
    }

    isRestored.value = true

    clampToMonitor()
  }

  return {
    isRestored,
    restoreState,
  }
}
