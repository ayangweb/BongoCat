import { PhysicalPosition } from '@tauri-apps/api/dpi'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { useDebounceFn } from '@vueuse/core'
import { onMounted, ref, watch } from 'vue'

import { useCatStore } from '@/stores/cat'
import { useModelStore } from '@/stores/model'
import { getCursorMonitor } from '@/utils/monitor'

const appWindow = getCurrentWebviewWindow()

export function useWindowPosition() {
  const catStore = useCatStore()
  const modelStore = useModelStore()
  const isMounted = ref(false)

  const setPosition = async () => {
    const monitor = await getCursorMonitor()

    if (!monitor) return

    const windowSize = await appWindow.outerSize()

    switch (catStore.window.position) {
      case 'topLeft':
        return appWindow.setPosition(new PhysicalPosition(0, 0))
      case 'topRight':
        return appWindow.setPosition(new PhysicalPosition(monitor.size.width - windowSize.width, 0))
      case 'bottomLeft':
        return appWindow.setPosition(new PhysicalPosition(0, monitor.size.height - windowSize.height))
      default:
        return appWindow.setPosition(new PhysicalPosition(monitor.size.width - windowSize.width, monitor.size.height - windowSize.height))
    }
  }

  const debouncedSetPosition = useDebounceFn(setPosition, 100)

  onMounted(async () => {
    await setPosition()

    isMounted.value = true

    appWindow.onScaleChanged(debouncedSetPosition)
  })

  watch([() => catStore.window.position, () => catStore.window.scale, () => modelStore.currentModel], debouncedSetPosition, { deep: true })

  return {
    isMounted,
  }
}
