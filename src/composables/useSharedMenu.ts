import type { CatMode } from '@/stores/cat'

import { CheckMenuItem, MenuItem, PredefinedMenuItem, Submenu } from '@tauri-apps/api/menu'

import { hideWindow, showWindow } from '@/plugins/window'
import { useCatStore } from '@/stores/cat'
import { isMac } from '@/utils/platform'

interface ModeOption {
  label: string
  value: CatMode
}

export function useSharedMenu() {
  const catStore = useCatStore()
  const modeOptions: ModeOption[] = [
    { label: '标准模式', value: 'standard' },
    { label: '键盘模式', value: 'keyboard' },
  ]

  const getOpacityMenuItems = async () => {
    const options = [25, 50, 75, 100]

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
        action: () => showWindow('preference'),
      }),
      MenuItem.new({
        text: catStore.visible ? '隐藏猫咪' : '显示猫咪',
        action: () => {
          if (catStore.visible) {
            hideWindow('main')
          } else {
            showWindow('main')
          }

          catStore.visible = !catStore.visible
        },
      }),
      PredefinedMenuItem.new({ item: 'Separator' }),
      Submenu.new({
        text: '猫咪模式',
        items: await Promise.all(
          modeOptions.map((item) => {
            return CheckMenuItem.new({
              text: item.label,
              checked: catStore.mode === item.value,
              action: () => {
                catStore.mode = item.value
              },
            })
          }),
        ),
      }),
      CheckMenuItem.new({
        text: '窗口穿透',
        checked: catStore.penetrable,
        action: () => {
          catStore.penetrable = !catStore.penetrable
        },
      }),
      Submenu.new({
        text: '不透明度',
        items: await getOpacityMenuItems(),
      }),
      CheckMenuItem.new({
        text: '镜像模式',
        checked: catStore.mirrorMode,
        action: () => {
          catStore.mirrorMode = !catStore.mirrorMode
        },
      }),
    ])
  }

  return {
    getSharedMenu,
  }
}
