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
</script>

<template>
  <div
    class="relative children:(absolute h-screen w-screen)"
    @mousedown="handleWindowDrag"
  >
    <div v-show="resizing" class="absolute inset-0 z-99 flex items-center justify-center bg-black">
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
