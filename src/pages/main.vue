<script setup lang="ts">
import type { KeyMap } from '../constants/keyMap'

import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { nextTick, onMounted, onUnmounted, ref, watch } from 'vue'

import { useDevice } from '../composables/useDevice'
import { useModel } from '../composables/useModel'
import { KEY_MAP } from '../constants/keyMap'

const { pressedKeys, pressedMouses, mousePosition } = useDevice()
const { handleLoadModel, handleDestroy, handleMouseClick, handleResized, handleMouseMove, handleKeyDown, handleSetMode, mode, background } = useModel()

const isOverLap = ref(false)
let resizeTimer: NodeJS.Timeout | null = null

const keyHistory = ref<string[]>([])

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

const displayDuration = ref(3000)
watch(pressedKeys, (value) => {
  handleKeyDown(value)

  const icons = value.map(key => KEY_MAP[key as KeyMap])
  keyHistory.value.push(...icons)

  setTimeout(() => {
    keyHistory.value.splice(0, icons.length)
  }, displayDuration.value)
})
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

    <div class="absolute left-0 right-0 top-1 z-9 flex items-center justify-center gap-2 h-8!">
      <div class="ml-2 h-full flex flex-1 items-center justify-center overflow-hidden rounded-md bg-black/50 p-2">
        <span v-for="icon in keyHistory" :key="icon" :class="icon" class="text-2xl text-white">
          {{ icon }}
        </span>
      </div>
      <button class="shrink-0 rounded-full bg-sky text-center text-white h-8! w-20! hover:shadow-lg" @click.stop="handleSwitch">
        切换模式
      </button>
    </div>

    <img :src="background">

    <canvas id="live2dCanvas" />

    <template v-for="key in pressedKeys" :key="key">
      <img :src="`/images/keys/${key}.png`">
      <img :src="`/images/hands/${key}.png`">
    </template>
  </div>
</template>
