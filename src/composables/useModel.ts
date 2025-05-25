import { LogicalSize, PhysicalSize } from '@tauri-apps/api/dpi'
import { resolveResource } from '@tauri-apps/api/path'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { message } from 'ant-design-vue'
import { round } from 'es-toolkit'
import { watch } from 'vue'

import live2d from '../utils/live2d'
import { getCursorMonitor } from '../utils/monitor'

import { DEFAULT_MODEL_HEIGHT, DEFAULT_MODEL_WIDTH } from '@/constants'
import { useCatStore } from '@/stores/cat'
import { useModelStore } from '@/stores/model'

const appWindow = getCurrentWebviewWindow()

export function useModel() {
  const modelStore = useModelStore()
  const catStore = useCatStore()

  watch(() => modelStore.currentModel, handleLoad, { deep: true, immediate: true })

  watch(() => catStore.scale, async () => {
    appWindow.setSize(
      new PhysicalSize({
        width: round(DEFAULT_MODEL_WIDTH * (catStore.scale / 100)),
        height: round(DEFAULT_MODEL_HEIGHT * (catStore.scale / 100)),
      }),
    )
  }, { immediate: true })

  async function handleLoad() {
    try {
      if (!modelStore.currentModel) return

      const { path } = modelStore.currentModel

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

    live2d.model?.scale.set(innerWidth / DEFAULT_MODEL_WIDTH)

    if (round(innerWidth / innerHeight, 1) !== round(DEFAULT_MODEL_WIDTH / DEFAULT_MODEL_HEIGHT, 1)) {
      await appWindow.setSize(
        new LogicalSize({
          width: innerWidth,
          height: Math.ceil(innerWidth * (DEFAULT_MODEL_HEIGHT / DEFAULT_MODEL_WIDTH)),
        }),
      )
    }

    const size = await appWindow.size()

    catStore.scale = round((size.width / DEFAULT_MODEL_WIDTH) * 100)
  }

  function handleKeyDown(side: 'left' | 'right', pressed: boolean) {
    if (side === 'left') {
      live2d.setParameterValue('CatParamLeftHandDown', pressed)
    } else {
      live2d.setParameterValue('CatParamRightHandDown', pressed)
    }
  }

  async function handleMouseMove() {
    const monitor = await getCursorMonitor()

    if (!monitor) return

    const { size, position, cursorPosition } = monitor

    const xRatio = (cursorPosition.x - position.x) / size.width
    const yRatio = (cursorPosition.y - position.y) / size.height

    const x = 30 - (xRatio * 60)
    const y = 30 - (yRatio * 60)

    live2d.setParameterValue('ParamMouseX', x)
    live2d.setParameterValue('ParamMouseY', y)

    live2d.setParameterValue('ParamAngleX', x)
    live2d.setParameterValue('ParamAngleY', y)
  }

  function handleMouseDown(value: string[]) {
    const hasLeftDown = value.includes('Left')
    const hasRightDown = value.includes('Right')

    live2d.setParameterValue('ParamMouseLeftDown', hasLeftDown)
    live2d.setParameterValue('ParamMouseRightDown', hasRightDown)
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
