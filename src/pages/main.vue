<script setup lang="ts">
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { useDebounceFn, useEventListener } from '@vueuse/core'
import { onMounted, onUnmounted, ref, watch } from 'vue'

import { useDevice } from '../composables/useDevice'
import { useModel } from '../composables/useModel'

const { pressedMouses, mousePosition, pressedKeys } = useDevice()
const { background, handleLoad, handleDestroy, handleResize, handleMouseDown, handleMouseMove, handleKeyDown } = useModel()

const resizing = ref(false)

onMounted(handleLoad)

onUnmounted(handleDestroy)

const handleDebounceResize = useDebounceFn(async () => {
  await handleResize()

  resizing.value = false
}, 100)

useEventListener('resize', () => {
  resizing.value = true

  handleDebounceResize()
})

watch(pressedMouses, handleMouseDown)

watch(mousePosition, handleMouseMove)

watch(pressedKeys, handleKeyDown)

function handleWindowDrag() {
  const appWindow = getCurrentWebviewWindow()

  appWindow.startDragging()
}

function resolveImageURL(key: string) {
  return new URL(`../assets/images/keys/${key}.png`, import.meta.url).href
}
</script>

<template>
  <div
    class="relative children:(absolute h-screen w-screen)"
    @mousedown="handleWindowDrag"
  >
    <img :src="background">

    <canvas id="live2dCanvas" />

    <img v-for="key in pressedKeys" :key="key" :src="resolveImageURL(key)">

    <div v-show="resizing" class="flex items-center justify-center bg-black">
      <span class="text-center text-5xl text-white">
        重绘中...
      </span>
    </div>
  </div>
</template>
