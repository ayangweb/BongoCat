import type { Event } from '@tauri-apps/api/event'

import { PhysicalPosition, PhysicalSize } from '@tauri-apps/api/dpi'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { onMounted, ref } from 'vue'

import { useAppStore } from '@/stores/app'

export type WindowState = Record<string, Partial<PhysicalPosition & PhysicalSize> | undefined>

const appWindow = getCurrentWebviewWindow()
const { label } = appWindow

export function useWindowState() {
  const appStore = useAppStore()
  const isRestored = ref(false)

  onMounted(() => {
    appWindow.onMoved(onChange)

    appWindow.onResized(onChange)
  })

  const onChange = async (event: Event<PhysicalPosition | PhysicalSize>) => {
    const minimized = await appWindow.isMinimized()

    if (minimized) return

    appStore.windowState[label] ??= {}

    Object.assign(appStore.windowState[label], event.payload)
  }

  const restoreState = async () => {
    const { x, y, width, height } = appStore.windowState[label] ?? {}

    if (x && y) {
      await appWindow.setPosition(new PhysicalPosition(x, y))
    }

    if (width && height) {
      await appWindow.setSize(new PhysicalSize(width, height))
    }

    isRestored.value = true
  }

  return {
    isRestored,
    restoreState,
  }
}
