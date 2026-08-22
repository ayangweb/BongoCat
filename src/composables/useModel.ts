import type { PhysicalPosition } from '@tauri-apps/api/dpi'

import { LogicalSize } from '@tauri-apps/api/dpi'
import { resolveResource, sep } from '@tauri-apps/api/path'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { message } from 'antdv-next'
import { isNil, round } from 'es-toolkit'
import { findKey, nth } from 'es-toolkit/compat'
import { ref } from 'vue'

import { useCatStore } from '@/stores/cat'
import { useModelStore } from '@/stores/model'
import { getCursorMonitor } from '@/utils/monitor'
import { isMac } from '@/utils/platform'

import modelRuntime from '../utils/model-runtime'

const appWindow = getCurrentWebviewWindow()
const digitKeys = '1234567890'.split('') as readonly string[]
const letterKeys = 'QWERTYUIOPASDFGHJKLZXCVBNM'.split('') as readonly string[]

export interface ModelSize {
  width: number
  height: number
}

export function useModel() {
  const modelStore = useModelStore()
  const catStore = useCatStore()
  const modelSize = ref<ModelSize>()
  let loadGeneration = 0

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
    const generation = ++loadGeneration
    const currentModel = modelStore.currentModel

    modelSize.value = void 0
    modelStore.currentMotions = []
    modelStore.currentExpressions = []

    if (!currentModel) return false

    const { id, path, renderer } = currentModel
    const isCurrent = () => {
      const model = modelStore.currentModel

      return generation === loadGeneration
        && model?.id === id
        && model.path === path
        && model.renderer === renderer
    }

    try {
      await resolveResource(path)

      if (!isCurrent()) return false

      const { width, height, motions, expressions } = await modelRuntime.load(path, renderer)

      if (!isCurrent()) return false

      const nextMotions = Object.entries(motions)
      const nextModelSize = { width, height }
      const nextShortcuts: Array<[string, string]> = []
      const behaviorIds: string[] = []

      for (const [groupName, items] of nextMotions) {
        for (const [index] of items.entries()) {
          behaviorIds.push(getMotionShortcutId(id, groupName, index))
        }
      }

      for (const [index] of expressions.entries()) {
        behaviorIds.push(getExpressionShortcutId(id, index))
      }

      for (const [index, id] of behaviorIds.entries()) {
        if (modelStore.shortcuts[id]) continue

        const shortcut = getBehaviorShortcut(index)

        if (!shortcut) continue

        nextShortcuts.push([id, shortcut])
      }

      if (!isCurrent()) return false

      modelSize.value = nextModelSize
      modelStore.currentMotions = nextMotions
      modelStore.currentExpressions = expressions

      for (const [shortcutId, shortcut] of nextShortcuts) {
        modelStore.shortcuts[shortcutId] = shortcut
      }

      if (!await handleResize(generation, nextModelSize)) return false

      return isCurrent()
    } catch (error) {
      if (isAbortError(error) || !isCurrent()) return false

      message.error(String(error))

      return false
    }
  }

  function handleDestroy() {
    ++loadGeneration
    modelRuntime.destroy()
  }

  async function handleResize(
    generation = loadGeneration,
    nextModelSize = modelSize.value,
  ) {
    if (!nextModelSize || generation !== loadGeneration) return false

    const { width, height } = nextModelSize

    if (innerWidth > 0 && innerHeight > 0
      && round(innerWidth / innerHeight, 1) !== round(width / height, 1)) {
      await appWindow.setSize(
        new LogicalSize({
          width: innerWidth,
          height: Math.ceil(innerWidth * (height / width)),
        }),
      )

      if (generation !== loadGeneration) return false
    }

    await new Promise<void>((resolve) => {
      requestAnimationFrame(() => resolve())
    })

    if (generation !== loadGeneration) return false

    modelRuntime.resizeModel(nextModelSize)

    const size = await appWindow.size()

    if (generation !== loadGeneration) return false

    catStore.window.scale = round((size.width / width) * 100)

    return true
  }

  const handlePress = (key: string, label?: string | null) => {
    modelRuntime.handleKeyboard(key, true, label)

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
    modelRuntime.handleKeyboard(key, false)

    delete modelStore.pressedKeys[key]
  }

  function handleKeyChange(isLeft = true, pressed = true) {
    const id = isLeft ? 'CatParamLeftHandDown' : 'CatParamRightHandDown'

    modelRuntime.setParameterValue(id, pressed)
  }

  function handleMouseChange(key: string, pressed = true) {
    const id = key === 'Left' ? 'ParamMouseLeftDown' : 'ParamMouseRightDown'

    modelRuntime.handleMouse(key, pressed)
    modelRuntime.setParameterValue(id, pressed)
  }

  async function handleMouseMove(cursorPoint: PhysicalPosition) {
    const monitor = await getCursorMonitor(cursorPoint)

    if (!monitor) return

    const { size, position } = monitor

    const xRatio = (cursorPoint.x - position.x) / size.width
    const yRatio = (cursorPoint.y - position.y) / size.height

    for (const id of [
      'ParamMouseX',
      'ParamMouseY',
      'ParamAngleX',
      'ParamAngleY',
      'ParamAngleZ',
      'ParamEyeBallX',
      'ParamEyeBallY',
    ]) {
      const range = modelRuntime.getParameterValueRange(id)

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

      modelRuntime.setParameterValue(id, value)
    }
  }

  async function handleAxisChange(id: string, value: number) {
    const range = modelRuntime.getParameterValueRange(id)

    if (!range) return

    const { min, max } = range

    modelRuntime.setParameterValue(id, Math.max(min, value * max))
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

function isAbortError(error: unknown) {
  return typeof error === 'object'
    && error !== null
    && 'name' in error
    && error.name === 'AbortError'
}
