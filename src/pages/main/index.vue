<script setup lang="ts">
import { convertFileSrc } from '@tauri-apps/api/core'
import { Menu } from '@tauri-apps/api/menu'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { useDebounceFn, useEventListener } from '@vueuse/core'
import { onUnmounted, ref, watch } from 'vue'

import { useDevice } from '@/composables/useDevice'
import { useModel } from '@/composables/useModel'
import { useSharedMenu } from '@/composables/useSharedMenu'
import { hideWindow, setAlwaysOnTop, showWindow } from '@/plugins/window'
import { useCatStore } from '@/stores/cat'
import { useModelStore } from '@/stores/model'
import { join } from '@/utils/path'
import { adsorb } from '@/utils/window'

const appWindow = getCurrentWebviewWindow()
const { pressedMouses, mousePosition, pressedLeftKeys, pressedRightKeys } = useDevice()
const { backgroundImage, handleDestroy, handleResize, handleMouseDown, handleMouseMove, handleKeyDown } = useModel()
const catStore = useCatStore()
const { getSharedMenu } = useSharedMenu()
const modelStore = useModelStore()
const resizing = ref(false)

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

watch(pressedLeftKeys, (keys) => {
  handleKeyDown('left', keys.length > 0)
})

watch(pressedRightKeys, (keys) => {
  handleKeyDown('right', keys.length > 0)
})

watch(() => catStore.visible, async (value) => {
  value ? showWindow() : hideWindow()
})

watch(() => catStore.penetrable, (value) => {
  appWindow.setIgnoreCursorEvents(value)
}, { immediate: true })

watch(() => catStore.alwaysOnTop, setAlwaysOnTop, { immediate: true })

function handleWindowDrag() {
  appWindow.startDragging()
}

async function handleContextmenu(event: MouseEvent) {
  event.preventDefault()

  const menu = await Menu.new({
    items: await getSharedMenu(),
  })

  menu.popup()
}

function handleMouseUp(event: MouseEvent) {
  if (event.button !== 0 || !catStore.autoAdsorb) return

  adsorb()
}

function resolveImagePath(key: string, side: 'left' | 'right' = 'left') {
  return convertFileSrc(join(modelStore.currentModel!.path, 'resources', `${side}-keys`, `${key}.png`))
}
</script>

<template>
  <div
    class="relative size-screen overflow-hidden children:(absolute size-full)"
    :class="[catStore.mirrorMode ? '-scale-x-100' : 'scale-x-100']"
    :style="{ opacity: catStore.opacity / 100 }"
    @contextmenu="handleContextmenu"
    @mousedown="handleWindowDrag"
    @mouseup="handleMouseUp"
  >
    <img :src="backgroundImage">

    <canvas id="live2dCanvas" />

    <img
      v-for="key in pressedLeftKeys"
      :key="key"
      :src="resolveImagePath(key)"
    >

    <img
      v-for="key in pressedRightKeys"
      :key="key"
      :src="resolveImagePath(key, 'right')"
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
