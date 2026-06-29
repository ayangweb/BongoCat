<script setup lang="ts">
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { onMounted } from 'vue'

const appWindow = getCurrentWebviewWindow()

onMounted(() => {
  // 整窗鼠标穿透：透明空白区点不到
  appWindow.setIgnoreCursorEvents(true)
})
</script>

<template>
  <div class="size-screen" />
</template>
