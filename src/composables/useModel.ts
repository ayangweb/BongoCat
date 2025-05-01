import { LogicalSize } from '@tauri-apps/api/dpi'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { round } from 'es-toolkit'
import { computed, watch } from 'vue'

import live2d from '../utils/live2d'
import { getCursorMonitor } from '../utils/monitor'

import { useTauriListen } from './useTauriListen'

import { LISTEN_KEY } from '@/constants'
import { useCatStore } from '@/stores/cat'
import { useModelStore } from '@/stores/model'
import { getImageSize } from '@/utils/dom'

export function useModel() {
  const catStore = useCatStore()
  const modelStore = useModelStore()

  watch(() => catStore.mode, handleLoad)

  const backgroundImagePath = computed(() => {
    return `/images/backgrounds/${catStore.mode}.png`
  })

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
    const { innerWidth, innerHeight } = window
    const { width, height } = await getImageSize(backgroundImagePath.value)

    live2d.model?.scale.set(innerWidth / width)

    if (round(innerWidth / innerHeight, 1) === round(width / height, 1)) return

    return appWindow.setSize(
      new LogicalSize({
        width: innerWidth,
        height: Math.ceil(innerWidth * (height / width)),
      }),
    )
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
    backgroundImagePath,
    handleLoad,
    handleDestroy,
    handleResize,
    handleKeyDown,
    handleMouseMove,
    handleMouseDown,
  }
}
