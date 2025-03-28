<script setup lang="ts">
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { nextTick, onMounted, onUnmounted, ref, watch } from 'vue'

import { useDevice } from '../composables/useDevice'
import { useModel } from '../composables/useModel'

const { pressedKeys, pressedMouses, mousePosition } = useDevice()
const { handleLoadModel, handleDestroy, handleMouseClick, handleResized, handleMouseMove, handleKeyDown, handleSetMode, mode, background } = useModel()

const isOverLap = ref(false)
let resizeTimer: NodeJS.Timeout | null = null

async function handleSwitch() {
  const newMode = mode.value === 'STANDARD' ? 'KEYBOARD' : 'STANDARD'
  await handleSetMode(newMode)
  handleResized()
}

function handleResize() {
  isOverLap.value = true

  if (resizeTimer) clearTimeout(resizeTimer)

  resizeTimer = setTimeout(async () => {
    await handleResized()
    isOverLap.value = false
  }, 100)
}

onMounted(async () => {
  await nextTick()
  await handleLoadModel()
  handleResized()

  window.addEventListener('resize', handleResize)
})

onUnmounted(() => {
  handleDestroy()

  window.removeEventListener('resize', handleResize)

  if (resizeTimer)
    clearTimeout(resizeTimer)
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
    <div v-if="isOverLap" class="absolute inset-0 z-99 flex items-center justify-center bg-black">
      <span class="text-center text-5xl text-white">
        重绘中...
      </span>
    </div>

    <button class="absolute left-0 top-0 z-9 rounded-full bg-sky text-center text-white h-8! w-20! hover:shadow-lg" @click.stop="handleSwitch">
      切换模式
    </button>

    <img :src="background">

    <canvas id="live2dCanvas" />

    <template v-for="key in pressedKeys" :key="key">
      <img :src="`/images/keys/${key}.png`">
      <img :src="`/images/hands/${key}.png`">
    </template>
  </div>
</template>
