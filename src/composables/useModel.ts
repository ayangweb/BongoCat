import { LogicalSize } from '@tauri-apps/api/dpi'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { computed, watch } from 'vue'

import { MODEL_BACKGROUND } from '../constants'
import { useModelStore } from '../stores/model'
import live2d from '../utils/live2d'
import { getCursorMonitor } from '../utils/monitor'

export function useModel() {
  const modelState = useModelStore()

  const background = computed(() => MODEL_BACKGROUND[modelState.mode])

  watch(() => modelState.mode, onLoad)

  async function onLoad() {
    await live2d.load(modelState.mode)

    onResized()
  }

  function onDestroy() {
    live2d.destroy()
  }

  async function onResized() {
    if (!live2d.currentModel) return

    const { innerWidth } = window

    await getCurrentWebviewWindow().setSize(
      new LogicalSize({
        width: innerWidth,
        height: innerWidth * (354 / 612),
      }),
    )

    live2d.currentModel?.scale.set(innerWidth / 612)
  }

  function onKeyDown(value: string[]) {
    const hasArrowKey = value.some(key => key.endsWith('Arrow'))
    const hasNonArrowKey = value.some(key => !key.endsWith('Arrow'))

    live2d.setParameterValue('CatParamRightHandDown', hasArrowKey)
    live2d.setParameterValue('CatParamLeftHandDown', hasNonArrowKey)
  }

  async function onMouseMove() {
    if (!live2d.currentModel) return

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

  function onMouseDown(value: string[]) {
    const hasLeftDown = value.includes('Left')
    const hasRightDown = value.includes('Right')

    live2d.setParameterValue('ParamMouseLeftDown', hasLeftDown)
    live2d.setParameterValue('ParamMouseRightDown', hasRightDown)
  }

  return {
    background,
    motions: live2d.currentMotions,
    expressions: live2d.currentExpressions,
    onLoad,
    onDestroy,
    onResized,
    onKeyDown,
    onMouseMove,
    onMouseDown,
  }
}
