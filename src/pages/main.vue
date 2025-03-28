<script setup lang="ts">
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { nextTick, onMounted, onUnmounted, watch } from 'vue'

import { useDevice } from '../composables/useDevice'
import { useModel } from '../composables/useModel'

const { pressedKeys, pressedMouses, mousePosition } = useDevice()
const { handleLoadModel, handleDestroy, handleMouseClick, handleResized, handleMouseMove, handleKeyDown, background } = useModel()

onMounted(async () => {
  window.addEventListener('resize', handleResized)
  await nextTick()
  await handleLoadModel()
  handleResized()
})

onUnmounted(() => {
  handleDestroy()
  window.removeEventListener('resize', handleResized)
})

watch(pressedKeys, handleKeyDown)
watch(mousePosition, handleMouseMove)
watch(pressedMouses, handleMouseClick)

function handleMouseDown() {
  const appWindow = getCurrentWebviewWindow()

  appWindow.startDragging()
}
</script>

<template>
  <div
    class="relative children:(absolute h-screen w-screen)"
    @mousedown="handleMouseDown"
  >
    <img :src="background">

    <canvas id="live2dCanvas" />

    <template v-for="key in pressedKeys" :key="key">
      <img :src="`/images/keys/${key}.png`">
      <img :src="`/images/hands/${key}.png`">
    </template>
  </div>
</template>
