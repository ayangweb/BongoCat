import { invoke } from '@tauri-apps/api/core'
import { PhysicalPosition } from '@tauri-apps/api/dpi'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'

import { INVOKE_KEY, LISTEN_KEY } from '../constants'

import { useModel } from './useModel'
import { useTauriListen } from './useTauriListen'

import { useCatStore } from '@/stores/cat'
import { useModelStore } from '@/stores/model'
import { inBetween } from '@/utils/is'
import { isWindows } from '@/utils/platform'

interface MouseButtonEvent {
  kind: 'MousePress' | 'MouseRelease'
  value: string
}

export interface CursorPoint {
  x: number
  y: number
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
const MOUSE_MOVE_FRAME_MS = 16

export function useDevice() {
  const appWindow = getCurrentWebviewWindow()
  const modelStore = useModelStore()
  const releaseTimers = new Map<string, ReturnType<typeof setTimeout>>()
  const catStore = useCatStore()
  const { handlePress, handleRelease, handleMouseChange, handleMouseMove } = useModel()
  let latestCursorPoint: PhysicalPosition | undefined
  let lastMouseMoveAt = 0
  let mouseMoveTimer: ReturnType<typeof setTimeout> | undefined

  const startListening = () => {
    invoke(INVOKE_KEY.START_DEVICE_LISTENING)
  }

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

  const updateHideOnHover = async (cursorPoint: PhysicalPosition) => {
    if (catStore.window.hideOnHover) {
      const [position, { width, height }] = await Promise.all([
        appWindow.outerPosition(),
        appWindow.innerSize(),
      ])

      const isInWindow = inBetween(cursorPoint.x, position.x, position.x + width)
        && inBetween(cursorPoint.y, position.y, position.y + height)

      document.body.style.setProperty('opacity', isInWindow ? '0' : 'unset')

      if (!catStore.window.passThrough) {
        appWindow.setIgnoreCursorEvents(isInWindow)
      }
    }
  }

  const scheduleMouseMove = () => {
    if (mouseMoveTimer) return

    const delay = Math.max(0, MOUSE_MOVE_FRAME_MS - (performance.now() - lastMouseMoveAt))

    mouseMoveTimer = setTimeout(() => {
      mouseMoveTimer = void 0
      void flushMouseMove()
    }, delay)
  }

  const flushMouseMove = () => {
    if (!latestCursorPoint) return

    const cursorPoint = latestCursorPoint

    lastMouseMoveAt = performance.now()
    latestCursorPoint = void 0

    void handleMouseMove(cursorPoint)
    void updateHideOnHover(cursorPoint)
  }

  const handleCursorMove = (cursorPoint: CursorPoint) => {
    latestCursorPoint = new PhysicalPosition(cursorPoint.x, cursorPoint.y)
    scheduleMouseMove()
  }

  const handleAutoRelease = (key: string, delay = 100) => {
    handlePress(key)

    if (releaseTimers.has(key)) {
      clearTimeout(releaseTimers.get(key))
    }

    const timer = setTimeout(() => {
      handleRelease(key)

      releaseTimers.delete(key)
    }, delay)

    releaseTimers.set(key, timer)
  }

  useTauriListen<DeviceEvent>(LISTEN_KEY.DEVICE_CHANGED, ({ payload }) => {
    const { kind, value } = payload

    if (kind === 'KeyboardPress' || kind === 'KeyboardRelease') {
      const nextValue = getSupportedKey(value)

      if (!nextValue) return

      if (nextValue === 'CapsLock') {
        return handleAutoRelease(nextValue)
      }

      if (kind === 'KeyboardPress') {
        if (isWindows) {
          const delay = catStore.model.autoReleaseDelay * 1000

          return handleAutoRelease(nextValue, delay)
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
        return handleCursorMove(value)
    }
  })

  return {
    startListening,
  }
}
