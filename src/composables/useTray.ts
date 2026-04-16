import type { TrayIconOptions } from '@tauri-apps/api/tray'

import { getName, getVersion } from '@tauri-apps/api/app'
import { emit } from '@tauri-apps/api/event'
import { Menu, MenuItem, PredefinedMenuItem } from '@tauri-apps/api/menu'
import { resolveResource } from '@tauri-apps/api/path'
import { TrayIcon } from '@tauri-apps/api/tray'
import { openUrl } from '@tauri-apps/plugin-opener'
import { watchDebounced } from '@vueuse/core'
import { watch } from 'vue'
import { useI18n } from 'vue-i18n'

import { useCatStore } from '@/stores/cat'
import { useGeneralStore } from '@/stores/general'

import { GITHUB_LINK, LISTEN_KEY } from '../constants'
import { showWindow } from '../plugins/window'
import { isMac } from '../utils/platform'
import { useAppMenu } from './useAppMenu'

const TRAY_ID = 'BONGO_CAT_TRAY'

export function useTray() {
  const catStore = useCatStore()
  const generalStore = useGeneralStore()
  const { getBaseMenu, getExitMenu } = useAppMenu()
  const { t } = useI18n()

  watch([() => catStore.window.visible, () => catStore.window.passThrough, () => generalStore.appearance.language], () => {
    updateTrayMenu()
  })

  watchDebounced([() => catStore.window.scale, () => catStore.window.opacity], () => {
    updateTrayMenu()
  }, { debounce: 200 })

  const getTrayById = () => {
    return TrayIcon.getById(TRAY_ID)
  }

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

  const getTrayMenu = async () => {
    const appVersion = await getVersion()

    const items = await Promise.all([
      ...await getBaseMenu(),
      PredefinedMenuItem.new({ item: 'Separator' }),
      MenuItem.new({
        text: t('composables.useTray.checkUpdate'),
        action: () => {
          showWindow()

          emit(LISTEN_KEY.UPDATE_APP)
        },
      }),
      MenuItem.new({
        text: t('composables.useTray.openSource'),
        action: () => openUrl(GITHUB_LINK),
      }),
      PredefinedMenuItem.new({ item: 'Separator' }),
      MenuItem.new({
        text: `v${appVersion}`,
        enabled: false,
      }),
      ...await getExitMenu(),
    ])

    return Menu.new({ items })
  }

  const updateTrayMenu = async () => {
    const tray = await getTrayById()

    if (!tray) return

    const menu = await getTrayMenu()

    tray.setMenu(menu)
  }

  watch(() => generalStore.app.trayVisible, async (visible) => {
    const tray = await getTrayById() ?? await createTray()

    if (!tray) return

    tray.setVisible(visible)
  }, { immediate: true })
}
