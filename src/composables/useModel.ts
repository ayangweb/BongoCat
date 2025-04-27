import { LogicalSize } from '@tauri-apps/api/dpi'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { watch } from 'vue'

import live2d from '../utils/live2d'
import { getCursorMonitor } from '../utils/monitor'

import { useTauriListen } from './useTauriListen'

import { LISTEN_KEY } from '@/constants'
import { useCatStore } from '@/stores/cat'
import { useModelStore } from '@/stores/model'

export function useModel() {
  const catStore = useCatStore()
  const modelStore = useModelStore()

  watch(() => catStore.mode, handleLoad)

  useTauriListen<number>(LISTEN_KEY.PLAY_EXPRESSION, ({ payload }) => {
    live2d.playExpressions(payload)
  })

  async function handleLoad() {
    const data = await live2d.load(`/models/${catStore.mode}/cat.model3.json`)

    handleResize()

    Object.assign(modelStore, data)
  }

  function handleDestroy() {
    live2d.destroy()
  }

  async function handleResize() {
    if (!live2d.model) return

    const appWindow = getCurrentWebviewWindow()
    const { innerWidth } = window

    await appWindow.setSize(
      new LogicalSize({
        width: innerWidth,
        height: innerWidth * (354 / 612),
      }),
    )

    live2d.model?.scale.set(innerWidth / 612)
  }

  async function handleMouseWheel(event: WheelEvent) {
    event.preventDefault()
    event.stopPropagation()

    const appWindow = getCurrentWebviewWindow()

    try {
      const currentSize = (await appWindow.outerSize()).toLogical(window.devicePixelRatio)

      const scaleFactor = event.deltaY > 0 ? 0.95 : 1.05

      const newWidth = Math.round(currentSize.width * scaleFactor)
      const newHeight = Math.round(currentSize.height * scaleFactor)

      await appWindow.setSize(new LogicalSize(
        Math.max(200, newWidth),
        Math.max(200, newHeight),
      ))
    } catch (error) {
      console.error('调整窗口大小失败:', error)
    }
  }

  function handleKeyDown(value: string[]) {
    const hasArrowKey = value.some(key => key.endsWith('Arrow'))
    const hasNonArrowKey = value.some(key => !key.endsWith('Arrow'))

    live2d.setParameterValue('CatParamRightHandDown', hasArrowKey)
    live2d.setParameterValue('CatParamLeftHandDown', hasNonArrowKey)
  }

  async function handleMouseMove() {
    if (catStore.mode !== 'standard' || !live2d.model) return

    const monitor = await getCursorMonitor()

    if (!monitor) return

    const { size, cursorX, cursorY } = monitor
    const { width, height } = size

    const xRatio = cursorX / width
    const yRatio = cursorY / height

    const x = (xRatio * 60) - 30
    const y = (yRatio * 60) - 30

    live2d.setParameterValue('ParamMouseX', -x)
    live2d.setParameterValue('ParamMouseY', -y)
    live2d.setParameterValue('ParamAngleX', x)
    live2d.setParameterValue('ParamAngleY', -y)
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
    handleMouseWheel,
  }
}
