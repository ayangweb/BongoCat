import { CheckMenuItem, MenuItem, PredefinedMenuItem, Submenu } from '@tauri-apps/api/menu'
import { range } from 'es-toolkit'

import { showWindow } from '@/plugins/window'
import { useCatStore } from '@/stores/cat'
import { isMac } from '@/utils/platform'

export function useSharedMenu() {
  const catStore = useCatStore()

  const getScaleMenuItems = async () => {
    const options = range(50, 151, 25)

    const items = options.map((item) => {
      return CheckMenuItem.new({
        // 默认 default
        text: item === 100 ? 'mặc định' : `${item}%`,
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
        // 偏好设置 Preferences
        text: 'Cài đặt',
        accelerator: isMac ? 'Cmd+,' : '',
        action: () => showWindow('preference'),
      }),
      MenuItem.new({
        // 隐藏猫咪 Hidden cat 显示猫咪 Show cats
        text: catStore.visible ? 'Con mèo ẩn' : 'Hiển thị mèo',
        action: () => {
          catStore.visible = !catStore.visible
        },
      }),
      PredefinedMenuItem.new({ item: 'Separator' }),
      CheckMenuItem.new({
        // 窗口穿透 Window penetration
        text: 'Thâm nhập cửa sổ',
        checked: catStore.penetrable,
        action: () => {
          catStore.penetrable = !catStore.penetrable
        },
      }),
      Submenu.new({
        // 窗口尺寸 Window size
        text: 'Kích thước cửa sổ',
        items: await getScaleMenuItems(),
      }),
      Submenu.new({
        // Opacity 不透明度
        text: 'Độ mờ',
        items: await getOpacityMenuItems(),
      }),
    ])
  }

  return {
    getSharedMenu,
  }
}
