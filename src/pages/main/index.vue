<script setup lang="ts">
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { useDebounceFn, useEventListener } from '@vueuse/core'
import { onMounted, onUnmounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'

import { useDevice } from '@/composables/useDevice'
import { useModel } from '@/composables/useModel'
import { useTray } from '@/composables/useTray'
import { useCatStore } from '@/stores/cat'

const appWindow = getCurrentWebviewWindow()
const { pressedMouses, mousePosition, pressedKeys } = useDevice()
const { backgroundImagePath, handleLoad, handleDestroy, handleResize, handleMouseDown, handleMouseMove, handleKeyDown } = useModel()
const catStore = useCatStore()
// const { getSharedMenu } = useSharedMenu()
const { getTrayMenu } = useTray()

const resizing = ref(false)

const { t } = useI18n()

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
}, { immediate: true })

function handleWindowDrag() {
  appWindow.startDragging()
}

async function handleContextmenu(event: MouseEvent) {
  event.preventDefault()
  const menu = await getTrayMenu()
  await menu.popup()
}

function resolveImageURL(key: string) {
  return new URL(`../../assets/images/keys/${key}.png`, import.meta.url).href
}
</script>

<template>
  <div
    class="relative size-screen overflow-hidden children:(absolute size-full)"
    :class="[catStore.mirrorMode ? '-scale-x-100' : 'scale-x-100']"
    :style="{ opacity: catStore.opacity / 100 }"
    @contextmenu="handleContextmenu"
    @mousedown="handleWindowDrag"
  >
    <img :src="backgroundImagePath">

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
        {{ t('main.redrawing') }}
      </span>
    </div>
  </div>
</template>
