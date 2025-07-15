import type { MouseMoveValue } from './useDevice.ts'
import type { Monitor } from '@tauri-apps/api/window'

import { LogicalSize, PhysicalSize } from '@tauri-apps/api/dpi'
import { resolveResource } from '@tauri-apps/api/path'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { availableMonitors as getAvailableMonitors } from '@tauri-apps/api/window'
import { message } from 'ant-design-vue'
import { isNil, round } from 'es-toolkit'
import { computed, onBeforeMount, ref, watch } from 'vue'

import live2d from '../utils/live2d'
import { getCursorMonitor } from '../utils/monitor'

import { useCatStore } from '@/stores/cat'
import { useModelStore } from '@/stores/model'

const appWindow = getCurrentWebviewWindow()

interface ModelSize {
  width: number
  height: number
}

export function useModel() {
  const modelStore = useModelStore()
  const catStore = useCatStore()
  const modelSize = ref<ModelSize>()
  const availableMonitors = ref<Monitor[]>([])

  const isOnlySingleMonitor = computed(() => availableMonitors.value.length === 1)

  onBeforeMount(async () => {
    availableMonitors.value = await getAvailableMonitors()
  })

  watch(() => modelStore.currentModel, handleLoad, { deep: true, immediate: true })

  watch([() => catStore.scale, modelSize], async () => {
    if (!modelSize.value) return

    const { width, height } = modelSize.value

    appWindow.setSize(
      new PhysicalSize({
        width: round(width * (catStore.scale / 100)),
        height: round(height * (catStore.scale / 100)),
      }),
    )
  }, { immediate: true })

  async function handleLoad() {
    try {
      if (!modelStore.currentModel) return

      const { path } = modelStore.currentModel

      await resolveResource(path)

      const { width, height, ...rest } = await live2d.load(path)

      modelSize.value = { width, height }

      handleResize()

      Object.assign(modelStore, rest)
    } catch (error) {
      message.error(String(error))
    }
  }

  function handleDestroy() {
    live2d.destroy()
  }

  async function handleResize() {
    if (!modelSize.value) return

    live2d.fitModel()

    const { width, height } = modelSize.value

    if (round(innerWidth / innerHeight, 1) !== round(width / height, 1)) {
      await appWindow.setSize(
        new LogicalSize({
          width: innerWidth,
          height: Math.ceil(innerWidth * (height / width)),
        }),
      )
    }

    const size = await appWindow.size()

    catStore.scale = round((size.width / width) * 100)
  }

  function handleKeyDown(side: 'left' | 'right', pressed: boolean) {
    const id = side === 'left' ? 'CatParamLeftHandDown' : 'CatParamRightHandDown'

    const { min, max } = live2d.getParameterRange(id)

    live2d.setParameterValue(id, pressed ? max : min)
  }

  async function _getCursorMonitor(mousePosition: MouseMoveValue) {
    return isOnlySingleMonitor.value
      ? { ...availableMonitors.value[0], cursorPosition: mousePosition }
      : await getCursorMonitor()
  }

  async function handleMouseMove(mousePosition: MouseMoveValue) {
    const monitor = await _getCursorMonitor(mousePosition)

    if (!monitor) return

    const { size, position, cursorPosition } = monitor

    const xRatio = (cursorPosition.x - position.x) / size.width
    const yRatio = (cursorPosition.y - position.y) / size.height

    for (const id of ['ParamMouseX', 'ParamMouseY', 'ParamAngleX', 'ParamAngleY']) {
      const { min, max } = live2d.getParameterRange(id)

      if (isNil(min) || isNil(max)) continue

      const isXAxis = id.endsWith('X')

      const ratio = isXAxis ? xRatio : yRatio
      let value = max - (ratio * (max - min))

      if (isXAxis && catStore.mouseMirror) {
        value *= -1
      }

      live2d.setParameterValue(id, value)
    }
  }

  function handleMouseDown(value: string[]) {
    const params = {
      ParamMouseLeftDown: value.includes('Left'),
      ParamMouseRightDown: value.includes('Right'),
    }

    for (const [id, pressed] of Object.entries(params)) {
      const { min, max } = live2d.getParameterRange(id)

      live2d.setParameterValue(id, pressed ? max : min)
    }
  }

  return {
    handleLoad,
    handleDestroy,
    handleResize,
    handleKeyDown,
    handleMouseMove,
    handleMouseDown,
  }
}
