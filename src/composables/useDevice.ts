import { ref } from 'vue'

import { LISTEN_KEY } from '../constants'

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

export function useDevice() {
  const pressedMouses = ref<MouseButtonValue[]>([])
  const mousePosition = ref<MouseMoveValue | undefined>()
  const pressedKeys = ref<string[]>([])

  const handlePress = <T>(array: T[], value: T) => {
    return [...new Set([...array, value])]
  }

  const handleRelease = <T>(array: T[], value: T) => {
    return array.filter(item => item !== value)
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
        pressedKeys.value = handlePress(pressedKeys.value, value)
        break
      case 'KeyboardRelease':
        pressedKeys.value = handleRelease(pressedKeys.value, value)
        break
    }
  })

  return {
    pressedMouses,
    pressedKeys,
    mousePosition,
  }
}
