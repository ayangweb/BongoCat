import { CheckMenuItem, MenuItem, PredefinedMenuItem, Submenu } from '@tauri-apps/api/menu'
import { range } from 'es-toolkit'

import { useAppStore } from '@/stores/app'
import { useCatStore } from '@/stores/cat'
import { isMac } from '@/utils/platform'

export function useSharedMenu() {
  const appStore = useAppStore()
  const catStore = useCatStore()

  const getScaleMenuItems = async () => {
    const options = range(50, 151, 25)

    const items = options.map((item) => {
      return CheckMenuItem.new({
        text: item === 100 ? '默认' : `${item}%`,
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
      MenuItem.new({
        text: '偏好设置...',
        accelerator: isMac ? 'Cmd+,' : '',
        action: () => {
          appStore.visiblePreference = true
        },
      }),
      MenuItem.new({
        text: appStore.visibleCat ? '隐藏猫咪' : '显示猫咪',
        action: () => {
          appStore.visibleCat = !appStore.visibleCat
        },
      }),
      PredefinedMenuItem.new({ item: 'Separator' }),
      CheckMenuItem.new({
        text: '窗口穿透',
        checked: catStore.penetrable,
        action: () => {
          catStore.penetrable = !catStore.penetrable
        },
      }),
      Submenu.new({
        text: '窗口尺寸',
        items: await getScaleMenuItems(),
      }),
      Submenu.new({
        text: '不透明度',
        items: await getOpacityMenuItems(),
      }),
    ])
  }

  return {
    getSharedMenu,
  }
}
