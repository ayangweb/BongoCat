<script setup lang="ts">
import { listen } from '@tauri-apps/api/event'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { onMounted } from 'vue'
import { RouterView } from 'vue-router'

import { LISTEN_KEY } from './constants'
import { hideWindow, showWindow } from './plugins/window'
import { useModelStore } from './stores/model'

const modelStore = useModelStore()

onMounted(() => {
  modelStore.$tauri.start()

  const appWindow = getCurrentWebviewWindow()

  listen(LISTEN_KEY.SHOW_WINDOW, ({ payload }) => {
    if (appWindow.label !== payload) return

    showWindow()
  })

  listen(LISTEN_KEY.HIDE_WINDOW, ({ payload }) => {
    if (appWindow.label !== payload) return

    hideWindow()
  })
})
</script>

<template>
  <RouterView />
</template>
