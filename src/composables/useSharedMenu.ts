import type { CatMode } from '@/stores/cat'

import { CheckMenuItem, MenuItem, PredefinedMenuItem, Submenu } from '@tauri-apps/api/menu'
import { range } from 'es-toolkit'
// import { watch } from 'vue' // 移除此处的 watch
import { useI18n } from 'vue-i18n'

import { hideWindow, showWindow } from '@/plugins/window'
// import { useAppStore } from '@/stores/app' // 如果不再需要 appStore 实例于此文件 (如此处的 watch 被移除)，可以移除
import { useCatStore } from '@/stores/cat'
import { isMac } from '@/utils/platform'

interface ModeOption {
  label: string
  value: CatMode
}

export function useSharedMenu() {
  const { t } = useI18n()
  const catStore = useCatStore()
  // const appStore = useAppStore() // 如果 watch 被移除，此处可能不再需要

  // 移除此 watch，因为它不能直接更新特定的上下文菜单实例
  // watch(() => appStore.locale, () => {
  //   if (window.__TAURI__) {
  //     getSharedMenu()
  //   }
  // })

  const getScaleMenuItems = async () => {
    const options = range(50, 151, 25)

    const items = options.map((item) => {
      return CheckMenuItem.new({
        text: item === 100 ? t('window.size.default') : t('common.percent', { value: item }),
        checked: catStore.scale === item,
        action: () => {
          catStore.scale = item
        },
      })
    })

    if (!options.includes(catStore.scale)) {
      items.unshift(CheckMenuItem.new({
        text: t('common.percent', { value: catStore.scale }),
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
        text: t('common.percent', { value: item }),
        checked: catStore.opacity === item,
        action: () => {
          catStore.opacity = item
        },
      })
    })

    if (!options.includes(catStore.opacity)) {
      items.unshift(CheckMenuItem.new({
        text: t('common.percent', { value: catStore.opacity }),
        checked: true,
        enabled: false,
      }))
    }

    return Promise.all(items)
  }

  const getSharedMenu = async () => {
    // 将 modeOptions 定义移到函数内部，确保每次调用时都使用最新的翻译
    const modeOptions: ModeOption[] = [
      { label: t('mode.standard'), value: 'standard' },
      { label: t('mode.keyboard'), value: 'keyboard' },
    ]

    return await Promise.all([
      MenuItem.new({
        text: t('menu.preference'),
        accelerator: isMac ? 'Cmd+,' : '',
        action: () => showWindow('preference'),
      }),
      MenuItem.new({
        text: catStore.visible ? t('menu.hideCat') : t('menu.showCat'),
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
        text: t('menu.catMode'),
        items: await Promise.all(
          modeOptions.map((item) => { // 这里的 item.label 将会是最新翻译的
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
        text: t('menu.windowPenetrable'),
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
      CheckMenuItem.new({
        text: t('menu.mirrorMode'),
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
