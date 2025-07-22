import type { CursorPoint } from '@/utils/monitor'

import { invoke } from '@tauri-apps/api/core'
import { useDebounceFn } from '@vueuse/core'
import { isEqual, mapValues } from 'es-toolkit'
import { ref } from 'vue'

import { INVOKE_KEY, LISTEN_KEY } from '../constants'

import { useModel } from './useModel'
import { useTauriListen } from './useTauriListen'

import { useModelStore } from '@/stores/model'

interface MouseButtonEvent {
  kind: 'MousePress' | 'MouseRelease'
  value: string
}

interface MouseMoveEvent {
  kind: 'MouseMove'
  value: CursorPoint
}

interface KeyboardEvent {
  kind: 'KeyboardPress' | 'KeyboardRelease'
  value: string
}

type DeviceEvent = MouseButtonEvent | MouseMoveEvent | KeyboardEvent

export function useDevice() {
  const modelStore = useModelStore()
  const lastCursorPoint = ref<CursorPoint>({ x: 0, y: 0 })
  const { handlePress, handleRelease, handleMouseChange, handleMouseMove } = useModel()

  const startListening = () => {
    invoke(INVOKE_KEY.START_DEVICE_LISTENING)
  }

  const debouncedRelease = useDebounceFn(handleRelease, 100)

  const getSupportedKey = (key: string) => {
    let nextKey = key

    const unsupportedKey = !modelStore.supportKeys[nextKey]

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

  const processMouseMove = (point: CursorPoint) => {
    const roundedValue = mapValues(point, Math.round)

    if (isEqual(lastCursorPoint.value, roundedValue)) return

    lastCursorPoint.value = roundedValue

    return handleMouseMove(point)
  }

  useTauriListen<DeviceEvent>(LISTEN_KEY.DEVICE_CHANGED, ({ payload }) => {
    const { kind, value } = payload

    if (kind === 'KeyboardPress' || kind === 'KeyboardRelease') {
      const nextValue = getSupportedKey(value)

      if (!nextValue) return

      if (nextValue === 'CapsLock') {
        handlePress(nextValue)

        return debouncedRelease(nextValue)
      }

      if (kind === 'KeyboardPress') {
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
    startListening,
  }
}
