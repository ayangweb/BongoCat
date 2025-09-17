import type { Theme } from '@tauri-apps/api/window'

import { defineStore } from 'pinia'
import { reactive, ref } from 'vue'

export interface GeneralStore {
  app: {
    autostart: boolean
    taskbarVisible: boolean
  }
  appearance: {
    theme: 'auto' | Theme
    isDark: boolean
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

  const init = () => {
    app.autostart = autostart.value
    app.taskbarVisible = taskbarVisibility.value

    appearance.theme = theme.value
    appearance.isDark = isDark.value

    update.autoCheck = autoCheckUpdate.value
  }

  return {
    app,
    appearance,
    update,
    init,
  }
})
