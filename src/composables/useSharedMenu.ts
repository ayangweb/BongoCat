import { CheckMenuItem, MenuItem, PredefinedMenuItem, Submenu } from '@tauri-apps/api/menu'
import { range } from 'es-toolkit'
import { useI18n } from 'vue-i18n'

import { showWindow } from '@/plugins/window'
import { useCatStore } from '@/stores/cat'
import { isMac } from '@/utils/platform'

export function useSharedMenu() {
  const catStore = useCatStore()
  const { t } = useI18n()

  const getScaleMenuItems = async () => {
    const options = range(50, 151, 25)

    const items = options.map((item) => {
      return CheckMenuItem.new({
        text: `${item}%`,
        checked: catStore.window.scale === item,
        action: () => {
          catStore.window.scale = item
        },
      })
    })

    if (!options.includes(catStore.window.scale)) {
      items.unshift(CheckMenuItem.new({
        text: `${catStore.window.scale}%`,
        checked: true,
        enabled: false,
      }))
    }

    return Promise.all(items)
  }

  const getOpacityMenuItems = async () => {
    const options = range(25, 101, 25)

    const items = options.map((item) => {
      return CheckMenuItem.new({
        text: `${item}%`,
        checked: catStore.window.opacity === item,
        action: () => {
          catStore.window.opacity = item
        },
      })
    })

    if (!options.includes(catStore.window.opacity)) {
      items.unshift(CheckMenuItem.new({
        text: `${catStore.window.opacity}%`,
        checked: true,
        enabled: false,
      }))
    }

    return Promise.all(items)
  }

  const getSharedMenu = async () => {
    return await Promise.all([
      MenuItem.new({
        text: t('composables.useSharedMenu.labels.preference'),
        accelerator: isMac ? 'Cmd+,' : '',
        action: () => showWindow('preference'),
      }),
      MenuItem.new({
        text: catStore.window.visible ? t('composables.useSharedMenu.labels.hideCat') : t('composables.useSharedMenu.labels.showCat'),
        action: () => {
          catStore.window.visible = !catStore.window.visible
        },
      }),
      PredefinedMenuItem.new({ item: 'Separator' }),
      CheckMenuItem.new({
        text: t('composables.useSharedMenu.labels.passThrough'),
        checked: catStore.window.passThrough,
        action: () => {
          catStore.window.passThrough = !catStore.window.passThrough
        },
      }),
      Submenu.new({
        text: t('composables.useSharedMenu.labels.windowSize'),
        items: await getScaleMenuItems(),
      }),
      Submenu.new({
        text: t('composables.useSharedMenu.labels.opacity'),
        items: await getOpacityMenuItems(),
      }),
    ])
  }

  return {
    getSharedMenu,
  }
}
