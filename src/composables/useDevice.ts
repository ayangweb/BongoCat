import { invoke } from '@tauri-apps/api/core'
import { PhysicalPosition } from '@tauri-apps/api/dpi'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { Ticker } from 'pixi.js'

import { INVOKE_KEY, LISTEN_KEY } from '../constants'

import { useModel } from './useModel'
import { useTauriListen } from './useTauriListen'

import { useCatStore } from '@/stores/cat'
import { useModelStore } from '@/stores/model'
import { inBetween } from '@/utils/is'
import { isMac, isWindows } from '@/utils/platform'

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

export function useDevice() {
  const modelStore = useModelStore()
  const releaseTimers = new Map<string, NodeJS.Timeout>()
  const catStore = useCatStore()
  const { handlePress, handleRelease, handleMouseChange, handleMouseMove } = useModel()
  let latestMousePosition: CursorPoint | null = null
  let isTickerRegistered = false

  // Cached DPI scale factor for coordinate conversion. On Windows/Linux rdev
  // already reports physical pixels so this stays 1; on macOS rdev reports
  // logical points and we multiply by scaleFactor to get physical pixels.
  let cachedScaleFactor = 1

  const startListening = () => {
    invoke(INVOKE_KEY.START_DEVICE_LISTENING)

    if (isTickerRegistered) return

    isTickerRegistered = true

    // Seed the macOS scale-factor cache once; it seldom changes at runtime.
    if (isMac) {
      getCurrentWebviewWindow().scaleFactor().then((sf) => {
        cachedScaleFactor = sf
      })
    }

    // Why fully synchronous: the previous fire-and-forget async approach
    // caused multiple IPC chains to overlap and complete out-of-order,
    // writing Live2D parameters with stale positions. Keeping the entire
    // callback synchronous (with background-cached monitor data) guarantees
    // parameters are written in strict frame order — matching the Mver C++
    // reference implementation.
    Ticker.shared.add(() => {
      if (!latestMousePosition) return

      const point = latestMousePosition

      latestMousePosition = null

      const x = point.x * cachedScaleFactor
      const y = point.y * cachedScaleFactor

      // Synchronous: handleMouseMove uses cached monitor info internally,
      // so no IPC round-trips happen here.
      handleMouseMove(new PhysicalPosition(x, y))

      // hideOnHover is async (IPC for window position/size) but does not
      // affect Live2D parameters, so fire-and-forget is safe here.
      if (catStore.window.hideOnHover) {
        void handleHideOnHover(x, y)
      }
    })
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

  // Async hide-on-hover check, separated from the synchronous parameter path
  // so that its IPC calls (outerPosition, innerSize) can never delay or
  // reorder Live2D parameter writes.
  const handleHideOnHover = async (x: number, y: number) => {
    const appWindow = getCurrentWebviewWindow()
    const position = await appWindow.outerPosition()
    const { width, height } = await appWindow.innerSize()

    const isInWindow = inBetween(x, position.x, position.x + width)
      && inBetween(y, position.y, position.y + height)

    document.body.style.setProperty('opacity', isInWindow ? '0' : 'unset')

    if (!catStore.window.passThrough) {
      appWindow.setIgnoreCursorEvents(isInWindow)
    }
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
        latestMousePosition = value
    }
  })

  return {
    startListening,
  }
}
