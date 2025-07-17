import type { Ref } from 'vue'

import { isEqual, mapValues, uniq } from 'es-toolkit'
import { ref, watch } from 'vue'

import { LISTEN_KEY } from '../constants'

import { useKeys } from './useKeys'
import { useModel } from './useModel'
import { useTauriListen } from './useTauriListen'

import { useCatStore } from '@/stores/cat'
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
  const catStore = useCatStore()
  const lastMousePoint = ref<MouseMoveValue>({ x: 0, y: 0 })
  const releaseTimers = new Map<string, NodeJS.Timeout>()
  const { handleKeyChange, handleMouseChange, handleMouseMove } = useModel()

  const { supportLeftKeys, supportRightKeys, pressedLeftKeys, pressedRightKeys } = useKeys()

  const handlePress = (keys: Ref<string[]>, value?: string) => {
    if (!value) return

    if (catStore.singleMode) {
      keys.value = [value]
    } else {
      keys.value = uniq(keys.value.concat(value))
    }
  }

  const handleRelease = (keys: Ref<string[]>, value?: string) => {
    if (!value) return

    keys.value = keys.value.filter(item => item !== value)
  }

  const getSupportedKey = (key: string) => {
    for (const side of ['left', 'right']) {
      let nextKey = key

      const supportKeys = side === 'left' ? supportLeftKeys.value : supportRightKeys.value

      const unsupportedKey = !supportKeys.includes(key)

      if (key.startsWith('F') && unsupportedKey) {
        nextKey = key.replace(/F(\d+)/, 'Fn')
      }

      for (const item of ['Meta', 'Shift', 'Alt', 'Control']) {
        if (key.startsWith(item) && unsupportedKey) {
          const regex = new RegExp(`^(${item}).*`)
          nextKey = key.replace(regex, '$1')
        }
      }

      if (!supportKeys.includes(nextKey)) continue

      return nextKey
    }
  }

  const handleScheduleRelease = (keys: Ref<string[]>, key: string, delay = 500) => {
    if (releaseTimers.has(key)) {
      clearTimeout(releaseTimers.get(key))
    }

    const timer = setTimeout(() => {
      handleRelease(keys, key)

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

      const isLeft = supportLeftKeys.value.includes(nextValue)

      const pressedKeys = isLeft ? pressedLeftKeys : pressedRightKeys

      if (nextValue === 'CapsLock') {
        handlePress(pressedKeys, nextValue)

        return handleScheduleRelease(pressedKeys, nextValue, 100)
      }

      if (kind === 'KeyboardPress') {
        if (isWindows) {
          handleScheduleRelease(pressedKeys, nextValue)
        }

        return handlePress(pressedKeys, nextValue)
      }

      return handleRelease(pressedKeys, nextValue)
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

  watch(pressedLeftKeys, (keys) => {
    handleKeyChange(true, keys.length > 0)
  })

  watch(pressedRightKeys, (keys) => {
    handleKeyChange(false, keys.length > 0)
  })

  return {
    supportLeftKeys,
    supportRightKeys,
    pressedLeftKeys,
    pressedRightKeys,
  }
}
