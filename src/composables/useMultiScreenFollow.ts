import type { Monitor } from '@tauri-apps/api/window'

import { PhysicalPosition } from '@tauri-apps/api/dpi'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { cursorPosition, monitorFromPoint } from '@tauri-apps/api/window'
import { useIntervalFn } from '@vueuse/core'
import { watch } from 'vue'

import { setMultiScreenFollow } from '@/plugins/window'
import { useCatStore } from '@/stores/cat'
import { useGeneralStore } from '@/stores/general'
import { isMac } from '@/utils/platform'

const POLL_INTERVAL_MS = 800

function monitorKey(mon: Monitor) {
  return `${mon.position.x},${mon.position.y},${mon.size.width},${mon.size.height}`
}

export function useMultiScreenFollow() {
  if (!isMac) return

  const generalStore = useGeneralStore()
  const catStore = useCatStore()
  const appWindow = getCurrentWebviewWindow()

  // 记忆每个屏幕上窗口最后停留的相对偏移（相对屏幕原点）。
  // 大屏 → 小屏时偏移会被裁剪到小屏边界，若直接用这个被裁剪过的偏移再换算回大屏，
  // 用户最初的位置就会丢失。缓存能在回到原屏幕时恢复用户实际设定的位置。
  const monitorOffsets = new Map<string, { x: number, y: number }>()

  const tick = async () => {
    if (!catStore.window.visible) return

    const [winPos, winSize, cursor, scaleFactor] = await Promise.all([
      appWindow.outerPosition(),
      appWindow.outerSize(),
      cursorPosition(),
      appWindow.scaleFactor(),
    ])

    const cursorLogical = cursor.toLogical(scaleFactor)
    const winCenterLogical = new PhysicalPosition(
      winPos.x + Math.floor(winSize.width / 2),
      winPos.y + Math.floor(winSize.height / 2),
    ).toLogical(scaleFactor)

    const [cursorMon, winMon] = await Promise.all([
      monitorFromPoint(cursorLogical.x, cursorLogical.y),
      monitorFromPoint(winCenterLogical.x, winCenterLogical.y),
    ])

    if (!cursorMon || !winMon) return

    // 始终更新当前所在屏幕的偏移记忆，捕捉用户在屏内手动拖动后的最新位置。
    monitorOffsets.set(monitorKey(winMon), {
      x: winPos.x - winMon.position.x,
      y: winPos.y - winMon.position.y,
    })

    const sameMonitor
      = winMon.position.x === cursorMon.position.x
        && winMon.position.y === cursorMon.position.y
        && winMon.size.width === cursorMon.size.width
        && winMon.size.height === cursorMon.size.height

    if (sameMonitor) return

    // 优先使用目标屏幕的历史偏移；首次进入则沿用源屏幕的偏移作为初值。
    const remembered = monitorOffsets.get(monitorKey(cursorMon))
    const offsetX = remembered?.x ?? (winPos.x - winMon.position.x)
    const offsetY = remembered?.y ?? (winPos.y - winMon.position.y)

    const minX = cursorMon.position.x
    const maxX = cursorMon.position.x + cursorMon.size.width - winSize.width
    const minY = cursorMon.position.y
    const maxY = cursorMon.position.y + cursorMon.size.height - winSize.height

    const targetX = Math.max(minX, Math.min(cursorMon.position.x + offsetX, maxX))
    const targetY = Math.max(minY, Math.min(cursorMon.position.y + offsetY, maxY))

    if (targetX === winPos.x && targetY === winPos.y) return

    await appWindow.setPosition(new PhysicalPosition(targetX, targetY))
  }

  const { pause, resume } = useIntervalFn(tick, POLL_INTERVAL_MS, { immediate: false })

  watch(
    () => generalStore.app.multiScreenFollow,
    async (enabled) => {
      await setMultiScreenFollow(enabled)

      if (enabled) {
        resume()
      } else {
        monitorOffsets.clear()
        pause()
      }
    },
    { immediate: true },
  )
}
