import type { WindowState } from '@/composables/useWindowState'

import { getName, getVersion } from '@tauri-apps/api/app'
import { PhysicalPosition, PhysicalSize } from '@tauri-apps/api/dpi'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { availableMonitors } from '@tauri-apps/api/window'
import { defineStore } from 'pinia'
import { reactive, ref } from 'vue'

export const useAppStore = defineStore('app', () => {
  const name = ref('')
  const version = ref('')
  const windowState = reactive<WindowState>({})

  const init = async () => {
    name.value = await getName()
    version.value = await getVersion()
  }

  const reset = async () => {
    const appWindow = getCurrentWebviewWindow()
    const { label } = appWindow

    const size = await appWindow.size()
    const monitors = await availableMonitors()

    if (!monitors.length) {
      // Fallback if we somehow cannot read monitors: just clear the saved state
      delete windowState[label]
      return
    }

    const primary = monitors[0]

    const maxX = primary.size.width - size.width
    const maxY = primary.size.height - size.height

    const x = primary.position.x + Math.max(0, Math.floor(maxX / 2))
    const y = primary.position.y + Math.max(0, Math.floor(maxY / 2))

    windowState[label] = {
      x,
      y,
      width: size.width,
      height: size.height,
    }

    await appWindow.setPosition(new PhysicalPosition(x, y))
    await appWindow.setSize(new PhysicalSize(size.width, size.height))
  }

  return {
    name,
    version,
    windowState,
    init,
    reset,
  }
})
