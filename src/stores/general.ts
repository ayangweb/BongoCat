import type { Theme } from '@tauri-apps/api/window'

import { defineStore } from 'pinia'
import { getLocale } from 'tauri-plugin-locale-api'
import { reactive, ref } from 'vue'

import { LANGUAGE } from '@/constants'

export type Language = typeof LANGUAGE[keyof typeof LANGUAGE]

export interface GeneralStore {
  app: {
    autostart: boolean
    taskbarVisible: boolean
  }
  appearance: {
    theme: 'auto' | Theme
    isDark: boolean
    language?: Language
  }
  update: {
    autoCheck: boolean
  }
}

export const useGeneralStore = defineStore('general', () => {
  /* ------------ 废弃字段（后续删除） ------------ */

  /** @deprecated 请使用 `update.autoCheck` */
  const autoCheckUpdate = ref(false)

  /** @deprecated 请使用 `app.autostart` */
  const autostart = ref(false)

  /** @deprecated 请使用 `app.taskbarVisible` */
  const taskbarVisibility = ref(false)

  /** @deprecated 请使用 `appearance.theme` */
  const theme = ref<'auto' | Theme>('auto')

  /** @deprecated 请使用 `appearance.isDark` */
  const isDark = ref(false)

  /** @deprecated 用于标识数据是否已迁移，后续版本将删除 */
  const migrated = ref(false)

  const app = reactive<GeneralStore['app']>({
    autostart: false,
    taskbarVisible: false,
  })

  const appearance = reactive<GeneralStore['appearance']>({
    theme: 'auto',
    isDark: false,
  })

  const update = reactive<GeneralStore['update']>({
    autoCheck: false,
  })

  const getLanguage = async () => {
    const locale = await getLocale<Language>()

    if (Object.values(LANGUAGE).includes(locale)) {
      return locale
    }

    return LANGUAGE.EN_US
  }

  const init = async () => {
    appearance.language ??= await getLanguage()

    if (migrated.value) return

    app.autostart = autostart.value
    app.taskbarVisible = taskbarVisibility.value

    appearance.theme = theme.value
    appearance.isDark = isDark.value

    update.autoCheck = autoCheckUpdate.value

    migrated.value = true
  }

  return {
    migrated,
    app,
    appearance,
    update,
    init,
  }
})
