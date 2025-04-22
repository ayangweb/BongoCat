<script setup lang="ts">
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { useDebounceFn, useEventListener } from '@vueuse/core'
import { onMounted, onUnmounted, ref, watch } from 'vue'

import { useDevice } from '@/composables/useDevice'
import { useModel } from '@/composables/useModel'
import { useCatStore } from '@/stores/cat'

const appWindow = getCurrentWebviewWindow()
const { pressedMouses, mousePosition, pressedKeys } = useDevice()
const { handleLoad, handleDestroy, handleResize, handleMouseDown, handleMouseMove, handleKeyDown } = useModel()
const catStore = useCatStore()

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

watch(() => catStore.penetrable, (value) => {
  appWindow.setIgnoreCursorEvents(value)
})

function handleWindowDrag() {
  appWindow.startDragging()
}

function resolveImageURL(key: string) {
  return new URL(`../../assets/images/keys/${key}.png`, import.meta.url).href
}
</script>

<template>
  <div
    class="relative children:(absolute h-screen w-screen)"
    :class="[catStore.mirrorMode ? '-scale-x-100' : 'scale-x-100']"
    :style="{ opacity: catStore.opacity / 100 }"
    @mousedown="handleWindowDrag"
  >
    <img :src="`/images/backgrounds/${catStore.mode}.png`">

    <canvas id="live2dCanvas" />

    <img v-for="key in pressedKeys" :key="key" :src="resolveImageURL(key)">

    <div v-show="resizing" class="flex items-center justify-center bg-black">
      <span class="text-center text-5xl text-white">
        重绘中...
      </span>
    </div>
  </div>
</template>
