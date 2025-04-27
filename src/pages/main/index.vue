<script setup lang="ts">
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { LogicalSize } from '@tauri-apps/api/window'
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

async function handleMouseWheel(event: WheelEvent) {
  event.preventDefault()
  event.stopPropagation()

  try {
    const currentSize = (await appWindow.outerSize()).toLogical(window.devicePixelRatio)

    const scaleFactor = event.deltaY > 0 ? 0.95 : 1.05

    const newWidth = Math.round(currentSize.width * scaleFactor)
    const newHeight = Math.round(currentSize.height * scaleFactor)

    await appWindow.setSize(new LogicalSize(
      Math.max(200, newWidth),
      Math.max(200, newHeight),
    ))
  } catch (error) {
    console.error('调整窗口大小失败:', error)
  }
}

useEventListener('wheel', handleMouseWheel, { passive: false })

watch(pressedMouses, handleMouseDown)

watch(mousePosition, handleMouseMove)

watch(pressedKeys, handleKeyDown)

watch(() => catStore.penetrable, (value) => {
  appWindow.setIgnoreCursorEvents(value)
}, { immediate: true })

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
    :style="{
      opacity: catStore.opacity / 100,
    }"
    @mousedown="handleWindowDrag"
  >
    <img :src="`/images/backgrounds/${catStore.mode}.png`">

    <canvas id="live2dCanvas" />

    <img
      v-for="key in pressedKeys"
      :key="key"
      :src="resolveImageURL(key)"
    >

    <div
      v-show="resizing"
      class="flex items-center justify-center bg-black"
    >
      <span class="text-center text-5xl text-white">
        重绘中...
      </span>
    </div>
  </div>
</template>
