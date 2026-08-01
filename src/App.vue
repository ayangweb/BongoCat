<script setup lang="ts">
import { HappyProvider } from '@antdv-next/happy-work-theme'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { error } from '@tauri-apps/plugin-log'
import { openUrl } from '@tauri-apps/plugin-opener'
import { useEventListener } from '@vueuse/core'
import { ConfigProvider, theme } from 'antdv-next'
import { isString } from 'es-toolkit'
import isURL from 'is-url'
import { onMounted, onUnmounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { RouterView } from 'vue-router'

import { useTauriListen } from './composables/useTauriListen'
import { useWindowState } from './composables/useWindowState'
import { LANGUAGE, LISTEN_KEY } from './constants'
import { getAntdLocale } from './locales/index.ts'
import { hideWindow, showWindow } from './plugins/window'
import { useAppStore } from './stores/app'
import { useCatStore } from './stores/cat'
import { useGeneralStore } from './stores/general'
import { useModelStore } from './stores/model'
import { useShortcutStore } from './stores/shortcut.ts'

const appStore = useAppStore()
const modelStore = useModelStore()
const catStore = useCatStore()
const generalStore = useGeneralStore()
const shortcutStore = useShortcutStore()
const appWindow = getCurrentWebviewWindow()
const { isRestored, restoreState } = useWindowState()
const { darkAlgorithm, defaultAlgorithm } = theme
const { locale } = useI18n()
const themeReady = ref(false)
let unlistenTheme: (() => void) | undefined

function applyThemeModel(isDark: boolean) {
  if (appWindow.label !== 'main') return
  if (generalStore.appearance.theme !== 'auto') return

  const modelId = isDark ? modelStore.darkModelId : modelStore.lightModelId
  const model = modelStore.models.find(item => item.id === modelId)

  if (!model || model.id === modelStore.currentModel?.id) return

  modelStore.modelReady = false
  modelStore.currentModel = model
}

function applyResolvedTheme(isDark: boolean) {
  generalStore.appearance.isDark = isDark
  document.documentElement.classList.toggle('dark', isDark)
  applyThemeModel(isDark)
}

async function applyTheme(value: 'auto' | 'light' | 'dark') {
  const nextTheme = value === 'auto' ? null : value

  await appWindow.setTheme(nextTheme)

  applyResolvedTheme((nextTheme ?? (await appWindow.theme())) === 'dark')
}

onMounted(async () => {
  await appStore.$tauri.start()
  await appStore.init()
  await modelStore.$tauri.start()
  await modelStore.init()
  await catStore.$tauri.start()
  catStore.init()
  await generalStore.$tauri.start()
  await generalStore.init()
  await shortcutStore.$tauri.start()

  themeReady.value = true
  await applyTheme(generalStore.appearance.theme)
  await restoreState()

  unlistenTheme = await appWindow.onThemeChanged(({ payload }) => {
    if (generalStore.appearance.theme !== 'auto') return

    applyResolvedTheme(payload === 'dark')
  })
})

onUnmounted(() => unlistenTheme?.())

watch(() => generalStore.appearance.theme, (value) => {
  if (!themeReady.value) return

  applyTheme(value)
})

watch([() => modelStore.lightModelId, () => modelStore.darkModelId], () => {
  if (!themeReady.value) return

  applyThemeModel(generalStore.appearance.isDark)
})

watch(() => generalStore.appearance.language, (value) => {
  locale.value = value ?? LANGUAGE.EN_US
})

useTauriListen(LISTEN_KEY.SHOW_WINDOW, ({ payload }) => {
  if (appWindow.label !== payload) return

  showWindow()
})

useTauriListen(LISTEN_KEY.HIDE_WINDOW, ({ payload }) => {
  if (appWindow.label !== payload) return

  hideWindow()
})

useEventListener('unhandledrejection', ({ reason }) => {
  const message = isString(reason) ? reason : reason instanceof Error ? `${reason.name}: ${reason.message}\n${reason.stack ?? ''}` : JSON.stringify(reason)

  error(message)
})

useEventListener('click', (event) => {
  const link = (event.target as HTMLElement).closest('a')

  if (!link) return

  const { href, target } = link

  if (target === '_blank') return

  event.preventDefault()

  if (!isURL(href)) return

  openUrl(href)
})
</script>

<template>
  <HappyProvider
    v-slot="{ wave }"
    enabled
  >
    <ConfigProvider
      :locale="getAntdLocale(generalStore.appearance.language)"
      :theme="{
        algorithm: generalStore.appearance.isDark ? darkAlgorithm : defaultAlgorithm,
      }"
      :wave="wave"
    >
      <RouterView v-if="isRestored" />
    </ConfigProvider>
  </HappyProvider>
</template>
