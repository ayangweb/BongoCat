import type { TrayIconOptions } from '@tauri-apps/api/tray'

import { getName, getVersion } from '@tauri-apps/api/app'
import { Menu, MenuItem, PredefinedMenuItem } from '@tauri-apps/api/menu'
import { resolveResource } from '@tauri-apps/api/path'
import { TrayIcon } from '@tauri-apps/api/tray'
import { openUrl } from '@tauri-apps/plugin-opener'
import { exit, relaunch } from '@tauri-apps/plugin-process'
import { onMounted, ref, watch } from 'vue'

import { hideWindow, showWindow } from '../plugins/window'
import { isMac } from '../utils/platform'

const TRAY_ID = 'BONGO_CAT_TRAY'

export function useTray() {
  const visible = ref(true)

  onMounted(() => {
    createTray()
  })

  watch(visible, () => {
    updateTrayMenu()
  })

  const createTray = async () => {
    const tray = await getTrayById()

    if (tray) return

    const appName = await getName()
    const appVersion = await getVersion()

    const menu = await getTrayMenu()

    const iconPath = isMac ? 'assets/tray-mac.png' : 'assets/tray.png'
    const icon = await resolveResource(iconPath)

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
      MenuItem.new({
        text: '偏好设置...',
        accelerator: 'Cmd+,',
        action: () => showWindow(),
      }),
      MenuItem.new({
        text: visible.value ? '隐藏猫咪' : '显示猫咪',
        action: () => {
          if (visible.value) {
            hideWindow('main')
          } else {
            showWindow('main')
          }

          visible.value = !visible.value
        },
      }),
      PredefinedMenuItem.new({ item: 'Separator' }),
      MenuItem.new({
        text: '检查更新',
      }),
      MenuItem.new({
        text: '开源地址',
        action: () => openUrl('https://github.com/ayangweb/BongoCat'),
      }),
      PredefinedMenuItem.new({ item: 'Separator' }),
      MenuItem.new({
        text: `版本 ${appVersion}`,
        enabled: false,
      }),
      MenuItem.new({
        text: '重启应用',
        action: relaunch,
      }),
      MenuItem.new({
        text: '退出应用',
        accelerator: 'Cmd+Q',
        action: () => exit(0),
      }),
    ])

    return Menu.new({ items })
  }

  const updateTrayMenu = async () => {
    const tray = await getTrayById()

    if (!tray) return

    const menu = await getTrayMenu()

    tray.setMenu(menu)
  }
}
