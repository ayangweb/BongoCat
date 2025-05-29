import type { Ref } from 'vue'

import { readDir } from '@tauri-apps/plugin-fs'
import { uniq } from 'es-toolkit'
import { reactive, ref, watch } from 'vue'

import { LISTEN_KEY } from '../constants'

import { useTauriListen } from './useTauriListen'

import { useCatStore } from '@/stores/cat'
import { useModelStore } from '@/stores/model'
import { isImage } from '@/utils/is'
import { join } from '@/utils/path'
import { isWindows } from '@/utils/platform'

interface MouseButtonEvent {
  kind: 'MousePress' | 'MouseRelease'
  value: string
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
  const supportLeftKeys = ref<string[]>([])
  const supportRightKeys = ref<string[]>([])
  const pressedMouses = ref<string[]>([])
  const mousePosition = reactive<MouseMoveValue>({ x: 0, y: 0 })
  const pressedLeftKeys = ref<string[]>([])
  const pressedRightKeys = ref<string[]>([])
  const catStore = useCatStore()
  const modelStore = useModelStore()
  const releaseTimers = new Map<string, NodeJS.Timeout>()

  watch(() => modelStore.currentModel, async (model) => {
    if (!model) return

    const keySides = [
      {
        side: 'left',
        supportKeys: supportLeftKeys,
        pressedKeys: pressedLeftKeys,
      },
      {
        side: 'right',
        supportKeys: supportRightKeys,
        pressedKeys: pressedRightKeys,
      },
    ]

    for await (const item of keySides) {
      const { side, supportKeys, pressedKeys } = item

      try {
        const files = await readDir(join(model.path, 'resources', `${side}-keys`))

        const imageFiles = files.filter(file => isImage(file.name))

        supportKeys.value = imageFiles.map((item) => {
          return item.name.split('.')[0]
        })

        pressedKeys.value = pressedKeys.value.filter((key) => {
          return supportKeys.value.includes(key)
        })
      } catch {
        supportKeys.value = []
        pressedKeys.value = []
      }
    }
  }, { deep: true, immediate: true })

  const handlePress = (array: Ref<string[]>, value?: string) => {
    if (!value) return

    if (catStore.singleMode) {
      array.value = [value]
    } else {
      array.value = uniq(array.value.concat(value))
    }
  }

  const handleRelease = (array: Ref<string[]>, value?: string) => {
    if (!value) return

    array.value = array.value.filter(item => item !== value)
  }

  const getSupportedKey = (key: string) => {
    for (const side of ['left', 'right']) {
      let nextKey = key

      const supportKeys = side === 'left' ? supportLeftKeys.value : supportRightKeys.value

      const unsupportedKeys = !supportKeys.includes(key)

      if (key.startsWith('F') && unsupportedKeys) {
        nextKey = key.replace(/F(\d+)/, 'Fn')
      }

      for (const item of ['Meta', 'Shift', 'Alt', 'Control']) {
        if (key.startsWith(item) && unsupportedKeys) {
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

  useTauriListen<DeviceEvent>(LISTEN_KEY.DEVICE_CHANGED, ({ payload }) => {
    const { kind, value } = payload

    if (kind === 'KeyboardPress' || kind === 'KeyboardRelease') {
      const nextValue = getSupportedKey(value)

      if (!nextValue) return

      const isLeftSide = supportLeftKeys.value.includes(nextValue)

      const pressedKeys = isLeftSide ? pressedLeftKeys : pressedRightKeys

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
        return handlePress(pressedMouses, value)
      case 'MouseRelease':
        return handleRelease(pressedMouses, value)
      case 'MouseMove':
        return Object.assign(mousePosition, value)
    }
  })

  return {
    pressedMouses,
    mousePosition,
    pressedLeftKeys,
    pressedRightKeys,
  }
}
