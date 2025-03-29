<script setup lang="ts">
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { error } from '@tauri-apps/plugin-log'
import { useEventListener } from '@vueuse/core'
import { isString } from 'radash'
import { onMounted } from 'vue'
import { RouterView } from 'vue-router'

import { useTauriListen } from './composables/useTauriListen'
import { LISTEN_KEY } from './constants'
import { hideWindow, showWindow } from './plugins/window'
import { useModelStore } from './stores/model'

const modelStore = useModelStore()

const appWindow = getCurrentWebviewWindow()

onMounted(() => {
  modelStore.$tauri.start()
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
</script>

<template>
  <RouterView />
</template>
