import type { PhysicalPosition } from '@tauri-apps/api/dpi'

import { LogicalSize } from '@tauri-apps/api/dpi'
import { resolveResource, sep } from '@tauri-apps/api/path'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { message } from 'ant-design-vue'
import { isNil, round } from 'es-toolkit'
import { findKey, nth } from 'es-toolkit/compat'
import { ref } from 'vue'

import live2d from '../utils/live2d'

import { useCatStore } from '@/stores/cat'
import { useModelStore } from '@/stores/model'
import { getCursorMonitor } from '@/utils/monitor'
import { isMac } from '@/utils/platform'

const appWindow = getCurrentWebviewWindow()
const digitKeys = '1234567890'.split('') as readonly string[]
const letterKeys = 'QWERTYUIOPASDFGHJKLZXCVBNM'.split('') as readonly string[]

// ---------------------------------------------------------------------------
// Mouse-tracking smoothing state (module-level so it is shared across all
// composable instances and survives across re-renders).
//
// EMA formula:  smoothed = α × raw + (1 − α) × smoothed
//   α close to 1 → very responsive, little smoothing
//   α close to 0 → very smooth, more lag
//
// The actual α value is read from `catStore.model.mouseSmoothingAlpha` each
// frame so it can be changed at runtime without reloading the page.
// Default: 0.25 (see stores/cat.ts).  Recommended range: 0.1 – 0.5.
// ---------------------------------------------------------------------------
let smoothedXRatio = 0.5
let smoothedYRatio = 0.5
let isSmoothedInitialized = false

export interface ModelSize {
  width: number
  height: number
}

export function useModel() {
  const modelStore = useModelStore()
  const catStore = useCatStore()
  const modelSize = ref<ModelSize>()

  function getBehaviorShortcut(index: number) {
    const primary = isMac ? 'Command' : 'Control'

    const modifierGroups = [
      [primary],
      [primary, 'Shift'],
      [primary, 'Alt'],
      [primary, 'Shift', 'Alt'],
    ]

    const tiers = [
      ...modifierGroups.map(modifiers => ({ modifiers, keys: digitKeys })),
      ...modifierGroups.map(modifiers => ({ modifiers, keys: letterKeys })),
    ]

    let nextIndex = index

    for (const tier of tiers) {
      if (nextIndex < tier.keys.length) {
        return [...tier.modifiers, tier.keys[nextIndex]].join('+')
      }

      nextIndex -= tier.keys.length
    }

    return ''
  }

  function getMotionShortcutId(modelId: string, groupName: string, index: number) {
    return `${modelId}:motion:${groupName}:${index}`
  }

  function getExpressionShortcutId(modelId: string, index: number) {
    return `${modelId}:expression:${index}`
  }

  async function handleLoad() {
    try {
      if (!modelStore.currentModel) return

      const { path } = modelStore.currentModel

      await resolveResource(path)

      const { width, height, motions, expressions } = await live2d.load(path)

      const nextMotions = Object.entries(motions)

      modelSize.value = { width, height }
      modelStore.currentMotions = nextMotions
      modelStore.currentExpressions = expressions

      handleResize()

      const modelId = modelStore.currentModel.id

      const behaviorIds: string[] = []

      for (const [groupName, items] of nextMotions) {
        for (const [index] of items.entries()) {
          behaviorIds.push(getMotionShortcutId(modelId, groupName, index))
        }
      }

      for (const [index] of expressions.entries()) {
        behaviorIds.push(getExpressionShortcutId(modelId, index))
      }

      for (const [index, id] of behaviorIds.entries()) {
        if (modelStore.shortcuts[id]) continue

        const shortcut = getBehaviorShortcut(index)

        if (!shortcut) continue

        modelStore.shortcuts[id] = shortcut
      }
    } catch (error) {
      message.error(String(error))
    }
  }

  function handleDestroy() {
    live2d.destroy()
  }

  async function handleResize() {
    if (!modelSize.value) return

    live2d.resizeModel(modelSize.value)

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

    catStore.window.scale = round((size.width / width) * 100)
  }

  const handlePress = (key: string) => {
    const path = modelStore.supportKeys[key]

    if (!path) return

    const dirName = nth(path.split(sep()), -2)!
    const prevKey = findKey(modelStore.pressedKeys, (value) => {
      return value.includes(dirName)
    })

    if (prevKey) {
      handleRelease(prevKey)
    }

    modelStore.pressedKeys[key] = path
  }

  const handleRelease = (key: string) => {
    delete modelStore.pressedKeys[key]
  }

  function handleKeyChange(isLeft = true, pressed = true) {
    const id = isLeft ? 'CatParamLeftHandDown' : 'CatParamRightHandDown'

    live2d.setParameterValue(id, pressed)
  }

  function handleMouseChange(key: string, pressed = true) {
    const id = key === 'Left' ? 'ParamMouseLeftDown' : 'ParamMouseRightDown'

    live2d.setParameterValue(id, pressed)
  }

  async function handleMouseMove(cursorPoint: PhysicalPosition) {
    const monitor = await getCursorMonitor(cursorPoint)

    if (!monitor) return

    const { size, position } = monitor

    const rawXRatio = (cursorPoint.x - position.x) / size.width
    const rawYRatio = (cursorPoint.y - position.y) / size.height

    // Apply EMA smoothing to reduce jitter from high-report-rate mice.
    // On the very first call the smoothed values are seeded with the raw
    // values so the model snaps to the correct position instantly.
    const alpha = catStore.model.mouseSmoothingAlpha
    if (!isSmoothedInitialized) {
      smoothedXRatio = rawXRatio
      smoothedYRatio = rawYRatio
      isSmoothedInitialized = true
    } else {
      smoothedXRatio = alpha * rawXRatio + (1 - alpha) * smoothedXRatio
      smoothedYRatio = alpha * rawYRatio + (1 - alpha) * smoothedYRatio
    }

    const xRatio = smoothedXRatio
    const yRatio = smoothedYRatio

    for (const id of [
      'ParamMouseX',
      'ParamMouseY',
      'ParamAngleX',
      'ParamAngleY',
      'ParamAngleZ',
      'ParamEyeBallX',
      'ParamEyeBallY',
    ]) {
      const range = live2d.getParameterValueRange(id)

      if (!range) continue

      const { min, max } = range

      if (isNil(min) || isNil(max)) continue

      const isXAxis = id.endsWith('X')
      const isYAxis = id.endsWith('Y')
      const isZAxis = id.endsWith('Z')

      let value: number

      if (isZAxis) {
        const dragX = 1 - 2 * xRatio
        const dragY = 1 - 2 * yRatio

        value = dragX * dragY * min
      } else {
        const ratio = isXAxis ? xRatio : yRatio

        value = max - ratio * (max - min)
      }

      if (!isYAxis && catStore.model.mouseMirror) {
        value *= -1
      }

      live2d.setParameterValue(id, value)
    }
  }

  async function handleAxisChange(id: string, value: number) {
    const range = live2d.getParameterValueRange(id)

    if (!range) return

    const { min, max } = range

    live2d.setParameterValue(id, Math.max(min, value * max))
  }

  return {
    modelSize,
    handlePress,
    handleRelease,
    handleLoad,
    handleDestroy,
    handleResize,
    handleKeyChange,
    handleMouseChange,
    handleMouseMove,
    handleAxisChange,
  }
}
