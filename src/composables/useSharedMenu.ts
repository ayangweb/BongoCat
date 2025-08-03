import { CheckMenuItem, MenuItem, PredefinedMenuItem, Submenu } from '@tauri-apps/api/menu'
import { range } from 'es-toolkit'
import { useI18n } from 'vue-i18n'

import { showWindow } from '@/plugins/window'
import { useCatStore } from '@/stores/cat'
import { isMac } from '@/utils/platform'

export function useSharedMenu() {
  const catStore = useCatStore()
  // Use vue-i18n for menu item translations
  const { t } = useI18n()

  const getScaleMenuItems = async () => {
    const options = range(50, 151, 25)

    const items = options.map((item) => {
      return CheckMenuItem.new({
        text: item === 100 ? t('menu.default') : `${item}%`,
        checked: catStore.scale === item,
        action: () => {
          catStore.scale = item
        },
      })
    })

    if (!options.includes(catStore.scale)) {
      items.unshift(CheckMenuItem.new({
        text: `${catStore.scale}%`,
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
        checked: catStore.opacity === item,
        action: () => {
          catStore.opacity = item
        },
      })
    })

    if (!options.includes(catStore.opacity)) {
      items.unshift(CheckMenuItem.new({
        text: `${catStore.opacity}%`,
        checked: true,
        enabled: false,
      }))
    }

    return Promise.all(items)
  }

  const getSharedMenu = async () => {
    return await Promise.all([
      // Replace hardcoded text with i18n key
      MenuItem.new({
        text: t('menu.preference'),
        accelerator: isMac ? 'Cmd+,' : '',
        action: () => showWindow('preference'),
      }),
      MenuItem.new({
        text: catStore.visible ? t('menu.hideCat') : t('menu.showCat'),
        action: () => {
          catStore.visible = !catStore.visible
        },
      }),
      PredefinedMenuItem.new({ item: 'Separator' }),
      CheckMenuItem.new({
        text: t('menu.penetrable'),
        checked: catStore.penetrable,
        action: () => {
          catStore.penetrable = !catStore.penetrable
        },
      }),
      Submenu.new({
        text: t('menu.windowSize'),
        items: await getScaleMenuItems(),
      }),
      Submenu.new({
        text: t('menu.opacity'),
        items: await getOpacityMenuItems(),
      }),
    ])
  }

  return {
    getSharedMenu,
  }
}
