import type { ModelType } from '../constants'

import { LogicalSize } from '@tauri-apps/api/dpi'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { computed, ref } from 'vue'

import { MODEL_BACKGROUND } from '../constants'
import live2d from '../utils/live2d'
import { getCursorMonitor } from '../utils/monitor'

export function useModel() {
  const mode = ref<ModelType>(localStorage.getItem('mode') as ModelType ?? 'STANDARD')

  const background = computed(() => MODEL_BACKGROUND[mode.value])

  async function handleSetMode(value: ModelType) {
    mode.value = value
    localStorage.setItem('mode', value)
    await handleLoadModel()
  }

  async function handleLoadModel() {
    await live2d.load(mode.value).catch((error) => {
      console.error('模型加载失败:', error)
    })
  }

  async function handleDestroy() {
    await live2d.destroy().catch((error) => {
      console.error('模型销毁失败:', error)
    })
  }

  async function handleResized() {
    if (!live2d.currentModel) return

    try {
      const { innerWidth } = window

      await getCurrentWebviewWindow().setSize(
        new LogicalSize({
          width: innerWidth,
          height: innerWidth * (354 / 612),
        }),
      )

      live2d.currentModel?.scale.set(innerWidth / 612)
    } catch (error) {
      console.error('窗口调整大小出错:', error)
    }
  }

  async function handleKeyDown(value: string[]) {
    try {
      const hasArrowKey = value.some(key => key.endsWith('Arrow'))
      const hasNonArrowKey = value.some(key => !key.endsWith('Arrow'))

      live2d.setParameterValue('CatParamRightHandDown', hasArrowKey)
      live2d.setParameterValue('CatParamLeftHandDown', hasNonArrowKey)
    } catch (error) {
      console.error('按键捕获出错:', error)
    }
  }

  async function handleMouseMove() {
    if (!live2d.currentModel) return

    try {
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
    } catch (error) {
      console.error('鼠标移动捕获出错:', error)
    }
  }

  async function handleMouseClick(value: string[]) {
    try {
      const hasLeftClick = value.includes('Left')
      const hasRightClick = value.includes('Right')

      live2d.setParameterValue('ParamMouseLeftDown', hasLeftClick)
      live2d.setParameterValue('ParamMouseRightDown', hasRightClick)
    } catch (error) {
      console.error('鼠标点击捕获出错:', error)
    }
  }

  return {
    mode,
    background,
    motions: live2d.currentMotions,
    expressions: live2d.currentExpressions,
    handleSetMode,
    handleLoadModel,
    handleDestroy,
    handleResized,
    handleKeyDown,
    handleMouseMove,
    handleMouseClick,
  }
}
