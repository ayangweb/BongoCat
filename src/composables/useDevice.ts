import type { Ref } from 'vue'

import { readDir } from '@tauri-apps/plugin-fs'
import { uniq } from 'es-toolkit'
import { nextTick, reactive, ref, watch } from 'vue'

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
  const isUserActive = ref(true)
  let inactivityTimer: NodeJS.Timeout | null = null
  let lastManualModel: string | null = null

  function resetInactivityTimer() {
    if (inactivityTimer) {
      clearTimeout(inactivityTimer)
    }

    isUserActive.value = true

    if (modelStore.autoSwitchEnabled) {
      inactivityTimer = setTimeout(() => {
        isUserActive.value = false
        handleModelSwitch()
      }, modelStore.autoSwitchInterval * 1000)
    }
  }

  function handleModelSwitch() {
    if (!modelStore.autoSwitchEnabled || modelStore.models.length <= 1) return

    if (modelStore.targetModelIndex !== null) {
      const index = Math.min(modelStore.targetModelIndex, modelStore.models.length - 1)
      const targetModel = modelStore.models[index]
      lastManualModel = modelStore.currentModel?.id || null
      modelStore.currentModel = targetModel
      resetInactivityTimer()
    }
  }

  function handleUserActivity() {
    if (inactivityTimer) {
      clearTimeout(inactivityTimer)
      inactivityTimer = null
    }

    isUserActive.value = true

    if (lastManualModel) {
      const manualModel = modelStore.models.find(model => model.id === lastManualModel)
      if (manualModel) {
        modelStore.currentModel = manualModel
      }
    }

    if (modelStore.autoSwitchEnabled) {
      inactivityTimer = setTimeout(() => {
        isUserActive.value = false
        handleModelSwitch()
      }, modelStore.autoSwitchInterval * 1000)
    }
  }

  watch(() => modelStore.currentModel?.id, (newId, oldId) => {
    if (newId && isUserActive.value && newId !== oldId && !modelStore.autoSwitchEnabled) {
      lastManualModel = newId
    }
  }, { immediate: true })

  watch(() => modelStore.autoSwitchEnabled, (enabled) => {
    if (enabled) {
      resetInactivityTimer()
    } else {
      if (inactivityTimer) {
        clearTimeout(inactivityTimer)
        inactivityTimer = null
      }
      if (lastManualModel && modelStore.currentModel?.id !== lastManualModel) {
        const manualModel = modelStore.models.find(model => model.id === lastManualModel)
        if (manualModel) {
          modelStore.currentModel = manualModel
          nextTick()
        }
      }
    }
  })

  watch([pressedMouses, mousePosition, pressedLeftKeys, pressedRightKeys], () => {
    resetInactivityTimer()
  }, { deep: true })

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
    handleUserActivity()
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
    supportLeftKeys,
    supportRightKeys,
    pressedMouses,
    mousePosition,
    pressedLeftKeys,
    pressedRightKeys,
    isUserActive,
  }
}
