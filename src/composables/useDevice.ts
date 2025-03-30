import type { Ref } from 'vue'

import { reactive, ref } from 'vue'

import { LISTEN_KEY } from '../constants'
import { useModelStore } from '../stores/model'

import { useTauriListen } from './useTauriListen'

type MouseButtonValue = 'Left' | 'Right' | 'Middle'

interface MouseButtonEvent {
  kind: 'MousePress' | 'MouseRelease'
  value: MouseButtonValue
}

interface MouseMoveValue {
  x: number
  y: number
}

interface MouseMoveEvent {
  kind: 'MouseMove'
  value: MouseMoveValue
}

interface KeyboardEvent {
  kind: 'KeyboardPress' | 'KeyboardRelease'
  value: string
}

type DeviceEvent = MouseButtonEvent | MouseMoveEvent | KeyboardEvent

function getSupportKeys() {
  const files = import.meta.glob('../assets/images/keys/*.png', { eager: true })

  return Object.keys(files).map((path) => {
    return path.split('/').pop()?.replace('.png', '')
  })
}

const supportKeys = getSupportKeys()

export function useDevice() {
  const pressedMouses = ref<MouseButtonValue[]>([])
  const mousePosition = reactive<MouseMoveValue>({ x: 0, y: 0 })
  const pressedKeys = ref<string[]>([])
  const modelStore = useModelStore()

  const handlePress = <T>(array: Ref<T[]>, value?: T) => {
    if (!value) return

    array.value = [...new Set([...array.value, value])]
  }

  const handleRelease = <T>(array: Ref<T[]>, value?: T) => {
    if (!value) return

    array.value = array.value.filter(item => item !== value)
  }

  const normalizeKeyValue = (key: string) => {
    key = key.replace(/(Left|Right|Gr)$/, '').replace(/F(\d+)/, 'Fn')

    const isInvalidArrowKey = key.endsWith('Arrow') && modelStore.mode !== 'KEYBOARD'
    const isUnsupportedKey = !supportKeys.includes(key)

    if (isInvalidArrowKey || isUnsupportedKey) return

    return key
  }

  useTauriListen<DeviceEvent>(LISTEN_KEY.DEVICE_CHANGED, ({ payload }) => {
    const { kind, value } = payload

    switch (kind) {
      case 'MousePress':
        return handlePress(pressedMouses, value)
      case 'MouseRelease':
        return handleRelease(pressedMouses, value)
      case 'MouseMove':
        return Object.assign(mousePosition, value)
      case 'KeyboardPress':
        return handlePress(pressedKeys, normalizeKeyValue(value))
      case 'KeyboardRelease':
        return handleRelease(pressedKeys, normalizeKeyValue(value))
    }
  })

  return {
    pressedMouses,
    mousePosition,
    pressedKeys,
  }
}
