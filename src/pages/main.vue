<script setup lang="ts">
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { onMounted, onUnmounted, ref, watch } from 'vue'

import { useDevice } from '../composables/useDevice'
import { useModel } from '../composables/useModel'

const { pressedKeys, pressedMouses, mousePosition } = useDevice()
const { loadModel, destroyModel, handleMouseDown, handleResized, handleMouseMove, handleKeyDown, background } = useModel()

const isOverLap = ref(false)
let resizeTimer: NodeJS.Timeout | null = null

function handleResize() {
  isOverLap.value = true

  if (resizeTimer) clearTimeout(resizeTimer)

  resizeTimer = setTimeout(async () => {
    await handleResized()
    isOverLap.value = false
  }, 100)
}

onMounted(() => {
  loadModel()

  window.addEventListener('resize', handleResize)
})

onUnmounted(() => {
  destroyModel()

  window.removeEventListener('resize', handleResize)

  if (!resizeTimer) return

  clearTimeout(resizeTimer)
})

watch(pressedKeys, handleKeyDown)

watch(mousePosition, handleMouseMove)

watch(pressedMouses, handleMouseDown)

function onMouseDown() {
  const appWindow = getCurrentWebviewWindow()

  appWindow.startDragging()
}
</script>

<template>
  <div
    class="relative children:(absolute h-screen w-screen)"
    @mousedown="onMouseDown"
  >
    <div v-if="isOverLap" class="absolute inset-0 z-99 flex items-center justify-center bg-black">
      <span class="text-center text-5xl text-white">
        重绘中...
      </span>
    </div>

    <img :src="background">

    <canvas id="live2dCanvas" />

    <template v-for="key in pressedKeys" :key="key">
      <img :src="`/images/keys/${key}.png`">
      <img :src="`/images/hands/${key}.png`">
    </template>
  </div>
</template>
