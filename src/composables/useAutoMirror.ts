import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { availableMonitors } from '@tauri-apps/api/window'
import { onMounted, onUnmounted } from 'vue'

import { useCatStore } from '@/stores/cat'

const appWindow = getCurrentWebviewWindow()

export function useAutoMirror() {
  const catStore = useCatStore()
  let unlisten: (() => void) | null = null

  onMounted(async () => {
    unlisten = await appWindow.onMoved(updateMirrorByPosition)

    if (catStore.model.autoMirror) {
      await updateMirrorByPosition()
    }
  })

  onUnmounted(() => {
    unlisten?.()
  })

  async function updateMirrorByPosition() {
    if (!catStore.model.autoMirror) return

    try {
      const windowPos = await appWindow.outerPosition()
      const windowSize = await appWindow.outerSize()
      const monitors = await availableMonitors()

      const windowCenterX = windowPos.x + windowSize.width / 2
      const windowCenterY = windowPos.y + windowSize.height / 2

      const monitor = monitors.find(m =>
        windowCenterX >= m.position.x
        && windowCenterX < m.position.x + m.size.width
        && windowCenterY >= m.position.y
        && windowCenterY < m.position.y + m.size.height,
      )

      if (!monitor) return

      const monitorCenterX = monitor.position.x + monitor.size.width / 2

      catStore.model.mirror = windowCenterX < monitorCenterX
    } catch {
      // Silently ignore — non-critical feature
    }
  }
}
