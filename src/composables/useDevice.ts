import { ref } from 'vue'

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
  const mousePosition = ref<MouseMoveValue | undefined>()
  const pressedKeys = ref<string[]>([])
  const modelStore = useModelStore()

  const handlePress = <T>(array: T[], value?: T) => {
    if (!value) return array

    return [...new Set([...array, value])]
  }

  const handleRelease = <T>(array: T[], value?: T) => {
    if (!value) return array

    return array.filter(item => item !== value)
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
        pressedMouses.value = handlePress(pressedMouses.value, value)
        break
      case 'MouseRelease':
        pressedMouses.value = handleRelease(pressedMouses.value, value)
        break
      case 'MouseMove':
        mousePosition.value = value
        break
      case 'KeyboardPress':
        pressedKeys.value = handlePress(pressedKeys.value, normalizeKeyValue(value))
        break
      case 'KeyboardRelease':
        pressedKeys.value = handleRelease(pressedKeys.value, normalizeKeyValue(value))
        break
    }
  })

  return {
    pressedMouses,
    mousePosition,
    pressedKeys,
  }
}
