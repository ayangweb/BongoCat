<script setup lang="ts">
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { useEventListener } from '@vueuse/core'
import { onMounted, onUnmounted, ref, watch } from 'vue'

import { useDevice } from '../composables/useDevice'
import { useModel } from '../composables/useModel'

const { pressedMouses, mousePosition, pressedKeys } = useDevice()
const { onLoad, onDestroy, onResized, onKeyDown, onMouseMove, onMouseDown, background } = useModel()

const isOverLap = ref(false)
let resizeTimer: NodeJS.Timeout | null = null

function handleResize() {
  isOverLap.value = true

  if (resizeTimer) clearTimeout(resizeTimer)

  resizeTimer = setTimeout(async () => {
    await onResized()
    isOverLap.value = false
  }, 100)
}

onMounted(onLoad)

onUnmounted(() => {
  onDestroy()

  if (!resizeTimer) return

  clearTimeout(resizeTimer)
})

useEventListener('resize', handleResize)

watch(pressedMouses, onMouseDown)

watch(mousePosition, onMouseMove)

watch(pressedKeys, onKeyDown)

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
