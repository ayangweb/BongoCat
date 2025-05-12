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

  // 新增：监听 catStore.size 的变化，当用户调整大小时，调用 handleResize
  watch(() => catStore.size, () => {
    if (live2d.model) { // 确保模型已加载
      handleResize()
    }
  })

  const backgroundImagePath = computed(() => {
    return `/images/backgrounds/${catStore.mode}.png`
  })

  useTauriListen<number>(LISTEN_KEY.PLAY_EXPRESSION, ({ payload }) => {
    live2d.playExpressions(payload)
  })

  async function handleLoad() {
    const data = await live2d.load(`/models/${catStore.mode}/cat.model3.json`)

    // handleResize 会在模型加载后被调用，它将应用包括用户自定义大小在内的缩放
    handleResize()

    Object.assign(modelStore, data)
  }

  function handleDestroy() {
    live2d.destroy()
  }

  // async function handleResize() {
  //   if (!live2d.model) return

  //   const appWindow = getCurrentWebviewWindow()
  //   const { innerWidth, innerHeight } = window
  //   const { width, height } = await getImageSize(backgroundImagePath.value) // 背景图的原始宽高

  //   // 用户自定义的缩放因子 (假设 catStore.size 是百分比, e.g., 100 代表 100%)
  //   const userScaleFactor = catStore.size / 100

  //   // 适应窗口宽度的自动缩放因子
  //   const autoResizeScaleFactor = innerWidth / width

  //   // 应用合并后的缩放比例
  //   live2d.model.scale.set(autoResizeScaleFactor * userScaleFactor)

  //   // 保持窗口宽高比与背景图一致的逻辑不变
  //   if (round(innerWidth / innerHeight, 1) === round(width / height, 1)) return

  //   return appWindow.setSize(
  //     new LogicalSize({
  //       width: innerWidth,
  //       height: Math.ceil(innerWidth * (height / width)),
  //     }),
  //   )
  // }

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

    const { size, position, cursorPosition } = monitor

    const xRatio = (cursorPosition.x - position.x) / size.width
    const yRatio = (cursorPosition.y - position.y) / size.height

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
