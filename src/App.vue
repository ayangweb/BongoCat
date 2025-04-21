<script setup lang="ts">
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { error } from '@tauri-apps/plugin-log'
import { openUrl } from '@tauri-apps/plugin-opener'
import { useEventListener } from '@vueuse/core'
import isURL from 'is-url'
import { isString } from 'radash'
import { onMounted } from 'vue'
import { RouterView } from 'vue-router'

import { useTauriListen } from './composables/useTauriListen'
import { useThemeVars } from './composables/useThemeVars'
import { LISTEN_KEY } from './constants'
import { hideWindow, showWindow } from './plugins/window'
import { useCatStore } from './stores/cat'
import { useGeneralStore } from './stores/general'
import { useModelStore } from './stores/model'

const { generateColorVars } = useThemeVars()
const modelStore = useModelStore()
const catStore = useCatStore()
const generalStore = useGeneralStore()
const appWindow = getCurrentWebviewWindow()

onMounted(() => {
  generateColorVars()
  modelStore.$tauri.start()
  catStore.$tauri.start()
  generalStore.$tauri.start()
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
  const message = isString(reason) ? reason : JSON.stringify(reason)

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
  <RouterView />
</template>
