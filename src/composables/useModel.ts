import { convertFileSrc } from '@tauri-apps/api/core'
import { LogicalSize, PhysicalSize } from '@tauri-apps/api/dpi'
import { resolveResource } from '@tauri-apps/api/path'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { message } from 'ant-design-vue'
import { isNil, round } from 'es-toolkit'
import { computed, watch } from 'vue'

import live2d from '../utils/live2d'
import { getCursorMonitor } from '../utils/monitor'

import { useCatStore } from '@/stores/cat'
import { useModelStore } from '@/stores/model'
import { getImageSize } from '@/utils/dom'
import { join } from '@/utils/path'

const appWindow = getCurrentWebviewWindow()

export function useModel() {
  const modelStore = useModelStore()
  const catStore = useCatStore()

  const backgroundImage = computed(() => {
    return convertFileSrc(join(modelStore.currentModel!.path, 'resources', 'background.png'))
  })

  watch(() => modelStore.currentModel, handleLoad, { deep: true, immediate: true })

  watch(() => catStore.scale, async () => {
    const { width, height } = await getImageSize(backgroundImage.value)

    appWindow.setSize(
      new PhysicalSize({
        width: round(width * (catStore.scale / 100)),
        height: round(height * (catStore.scale / 100)),
      }),
    )
  }, { immediate: true })

  async function handleLoad() {
    try {
      const { path } = modelStore.currentModel!

      await resolveResource(path)

      const data = await live2d.load(path)

      handleResize()

      Object.assign(modelStore, data)
    } catch (error) {
      message.error(String(error))
    }
  }

  function handleDestroy() {
    live2d.destroy()
  }

  async function handleResize() {
    if (!live2d.model) return

    const { innerWidth, innerHeight } = window

    const { width, height } = await getImageSize(backgroundImage.value)

    live2d.model?.scale.set(innerWidth / width)

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

  async function handleMouseMove() {
    const monitor = await getCursorMonitor()

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
    backgroundImage,
    handleLoad,
    handleDestroy,
    handleResize,
    handleKeyDown,
    handleMouseMove,
    handleMouseDown,
  }
}
