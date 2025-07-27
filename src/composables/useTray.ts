import type { TrayIconOptions } from '@tauri-apps/api/tray'

import { getName, getVersion } from '@tauri-apps/api/app'
import { emit } from '@tauri-apps/api/event'
import { Menu, MenuItem, PredefinedMenuItem } from '@tauri-apps/api/menu'
import { resolveResource } from '@tauri-apps/api/path'
import { TrayIcon } from '@tauri-apps/api/tray'
import { openUrl } from '@tauri-apps/plugin-opener'
import { exit, relaunch } from '@tauri-apps/plugin-process'
import { watchDebounced } from '@vueuse/core'
import { watch } from 'vue'

import { showWindow } from '../plugins/window'
import { isMac } from '../utils/platform'

import { useSharedMenu } from './useSharedMenu'

import { GITHUB_LINK, LISTEN_KEY } from '@/constants'
import { useCatStore } from '@/stores/cat'

const TRAY_ID = 'BONGO_CAT_TRAY'

export function useTray() {
  const catStore = useCatStore()
  const { getSharedMenu } = useSharedMenu()

  watch([() => catStore.visible, () => catStore.penetrable], async () => {
    await updateTrayMenu()
  })

  watchDebounced([() => catStore.scale, () => catStore.opacity], async () => {
    await updateTrayMenu()
  }, { debounce: 200 })

  const createTray = async () => {
    const tray = await getTrayById()

    if (tray) return

    const appName = await getName()
    const appVersion = await getVersion()

    const menu = await getTrayMenu()

    const path = isMac ? 'assets/tray-mac.png' : 'assets/tray.png'
    const icon = await resolveResource(path)

    const options: TrayIconOptions = {
      menu,
      icon,
      id: TRAY_ID,
      tooltip: `${appName} v${appVersion}`,
      iconAsTemplate: true,
      menuOnLeftClick: true,
    }

    return TrayIcon.new(options)
  }

  const getTrayById = () => {
    return TrayIcon.getById(TRAY_ID)
  }

  const getTrayMenu = async () => {
    const appVersion = await getVersion()

    const items = await Promise.all([
      ...await getSharedMenu(),
      PredefinedMenuItem.new({ item: 'Separator' }),
      MenuItem.new({
        // 检查更新 Check for updates
        text: 'Kiểm tra cập nhật',
        action: () => {
          showWindow()

          emit(LISTEN_KEY.UPDATE_APP)
        },
      }),
      MenuItem.new({
        // 开源地址 Open source address
        text: 'Địa chỉ nguồn mở',
        action: () => openUrl(GITHUB_LINK),
      }),
      PredefinedMenuItem.new({ item: 'Separator' }),
      MenuItem.new({
        // Version 版本
        text: `Phiên bản ${appVersion}`,
        enabled: false,
      }),
      MenuItem.new({
        // 重启应用 Restart the app
        text: 'Khởi động lại ứng dụng',
        action: relaunch,
      }),
      MenuItem.new({
        // Exit the application  退出应用
        text: 'Thoát khỏi ứng dụng',
        accelerator: isMac ? 'Cmd+Q' : '',
        action: () => exit(0),
      }),
    ])

    return Menu.new({ items })
  }

  const updateTrayMenu = async () => {
    const tray = await getTrayById()

    if (!tray) return

    const menu = await getTrayMenu()

    await tray.setMenu(menu)
  }

  return {
    createTray,
  }
}
