import { isEqual, mapValues } from 'es-toolkit'
import { ref } from 'vue'

import { LISTEN_KEY } from '../constants'

import { useModel } from './useModel'
import { useModelKeys } from './useModelKeys'
import { useTauriListen } from './useTauriListen'

import { isWindows } from '@/utils/platform'

interface MouseButtonEvent {
  kind: 'MousePress' | 'MouseRelease'
  value: string
}

export interface MouseMoveValue {
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
  const lastMousePoint = ref<MouseMoveValue>({ x: 0, y: 0 })
  const releaseTimers = new Map<string, NodeJS.Timeout>()
  const { supportKeys, pressedKeys, handlePress, handleRelease } = useModelKeys()
  const { handleMouseChange, handleMouseMove } = useModel()

  const getSupportedKey = (key: string) => {
    let nextKey = key

    const unsupportedKey = !supportKeys[nextKey]

    if (key.startsWith('F') && unsupportedKey) {
      nextKey = key.replace(/F(\d+)/, 'Fn')
    }

    for (const item of ['Meta', 'Shift', 'Alt', 'Control']) {
      if (key.startsWith(item) && unsupportedKey) {
        const regex = new RegExp(`^(${item}).*`)
        nextKey = key.replace(regex, '$1')
      }
    }

    return nextKey
  }

  const handleScheduleRelease = (key: string, delay = 500) => {
    if (releaseTimers.has(key)) {
      clearTimeout(releaseTimers.get(key))
    }

    const timer = setTimeout(() => {
      handleRelease(key)

      releaseTimers.delete(key)
    }, delay)

    releaseTimers.set(key, timer)
  }

  const processMouseMove = (value: MouseMoveValue) => {
    const roundedValue = mapValues(value, Math.round)

    if (isEqual(lastMousePoint.value, roundedValue)) return

    lastMousePoint.value = roundedValue

    return handleMouseMove(roundedValue)
  }

  useTauriListen<DeviceEvent>(LISTEN_KEY.DEVICE_CHANGED, ({ payload }) => {
    const { kind, value } = payload

    if (kind === 'KeyboardPress' || kind === 'KeyboardRelease') {
      const nextValue = getSupportedKey(value)

      if (!nextValue) return

      if (nextValue === 'CapsLock') {
        handlePress(nextValue)

        return handleScheduleRelease(nextValue, 100)
      }

      if (kind === 'KeyboardPress') {
        if (isWindows) {
          handleScheduleRelease(nextValue)
        }

        return handlePress(nextValue)
      }

      return handleRelease(nextValue)
    }

    switch (kind) {
      case 'MousePress':
        return handleMouseChange(value)
      case 'MouseRelease':
        return handleMouseChange(value, false)
      case 'MouseMove':
        return processMouseMove(value)
    }
  })

  return {
    pressedKeys,
  }
}
