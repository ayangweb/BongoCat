import type { Event } from '@tauri-apps/api/event'

import { PhysicalPosition, PhysicalSize } from '@tauri-apps/api/dpi'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { availableMonitors } from '@tauri-apps/api/window'
import { useDebounceFn } from '@vueuse/core'
import { isNumber } from 'es-toolkit/compat'
import { onUnmounted, ref, watch } from 'vue'

import { WINDOW_LABEL } from '@/constants'
import { useAppStore } from '@/stores/app'
import { useCatStore } from '@/stores/cat'
import { getCursorMonitor } from '@/utils/monitor'

export type WindowState = Record<string, Partial<PhysicalPosition & PhysicalSize> | undefined>

export const DEFAULT_PREFERENCE_WINDOW_SIZE = Object.freeze({
  width: 800,
  height: 600,
})

const appWindow = getCurrentWebviewWindow()
const { label } = appWindow

export function useWindowState() {
  const appStore = useAppStore()
  const catStore = useCatStore()
  const isRestored = ref(false)
  let tracking = false
  let disposed = false
  let restorePromise: Promise<void> | undefined
  let unlistenMoved: (() => void) | undefined
  let unlistenResized: (() => void) | undefined
  let unlistenScaleChanged: (() => void) | undefined

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
    if (!tracking || !isRestored.value || disposed) return

    const minimized = await appWindow.isMinimized()

    if (minimized || !tracking || disposed) return

    appStore.windowState[label] ??= {}

    Object.assign(appStore.windowState[label], event.payload)

    clampToMonitor()
  }

  const restoreState = () => {
    if (restorePromise) return restorePromise

    restorePromise = (async () => {
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

      const hasSavedSize = isNumber(width) && width > 0 && isNumber(height) && height > 0

      if (hasSavedSize) {
        await appWindow.setSize(new PhysicalSize(width, height))
      } else if (label === WINDOW_LABEL.PREFERENCE) {
        await appWindow.setSize(new PhysicalSize(DEFAULT_PREFERENCE_WINDOW_SIZE))
      }

      if (disposed) return

      // Native windows can emit their initial geometry while the store is still
      // hydrating. Start observing only after the persisted/default size wins.
      const unlisteners = await Promise.all([
        appWindow.onMoved(onChange),
        appWindow.onResized(onChange),
        appWindow.onScaleChanged(clampToMonitor),
      ])
      if (disposed) {
        unlisteners.forEach(unlisten => unlisten())
        return
      }

      unlistenMoved = unlisteners[0]
      unlistenResized = unlisteners[1]
      unlistenScaleChanged = unlisteners[2]
      tracking = true
      isRestored.value = true

      clampToMonitor()
    })()

    return restorePromise
  }

  onUnmounted(() => {
    disposed = true
    tracking = false
    unlistenMoved?.()
    unlistenResized?.()
    unlistenScaleChanged?.()
  })

  return {
    isRestored,
    restoreState,
  }
}
