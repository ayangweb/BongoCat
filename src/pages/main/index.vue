<script setup lang="ts">
import { convertFileSrc, invoke } from '@tauri-apps/api/core'
import { Menu } from '@tauri-apps/api/menu'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { exists } from '@tauri-apps/plugin-fs'
import { useDebounceFn, useEventListener } from '@vueuse/core'
import { onMounted, onUnmounted, ref, watch } from 'vue'

import { useDevice } from '@/composables/useDevice'
import { useModel } from '@/composables/useModel'
import { useSharedMenu } from '@/composables/useSharedMenu'
import { INVOKE_KEY } from '@/constants'
import { hideWindow, setAlwaysOnTop, showWindow } from '@/plugins/window'
import { useCatStore } from '@/stores/cat'
import { useModelStore } from '@/stores/model'
import { join } from '@/utils/path'

const appWindow = getCurrentWebviewWindow()
const { pressedMouses, mousePosition, pressedLeftKeys, pressedRightKeys } = useDevice()
const { handleDestroy, handleResize, handleMouseDown, handleMouseMove, handleKeyDown } = useModel()
const catStore = useCatStore()
const { getSharedMenu } = useSharedMenu()
const modelStore = useModelStore()
const resizing = ref(false)
const backgroundImagePath = ref<string>()

onMounted(() => {
  invoke(INVOKE_KEY.START_DEVICE_LISTENING)
})

onUnmounted(handleDestroy)

const debouncedResize = useDebounceFn(async () => {
  await handleResize()

  resizing.value = false
}, 100)

useEventListener('resize', () => {
  resizing.value = true

  debouncedResize()
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

watch(() => modelStore.currentModel, async (model) => {
  if (!model) return

  const path = join(model.path, 'resources', 'background.png')

  const existed = await exists(path)

  backgroundImagePath.value = existed ? convertFileSrc(path) : void 0
}, { deep: true, immediate: true })

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

function resolveKeyImagePath(key: string, side: 'left' | 'right' = 'left') {
  return convertFileSrc(join(modelStore.currentModel!.path, 'resources', `${side}-keys`, `${key}.png`))
}
</script>

<template>
  <div
    class="relative size-screen overflow-hidden children:(absolute size-full)"
    :class="{ '-scale-x-100': catStore.mirrorMode }"
    :style="{ opacity: catStore.opacity / 100 }"
    @contextmenu="handleContextmenu"
    @mousedown="handleWindowDrag"
  >
    <img
      v-if="backgroundImagePath"
      :src="backgroundImagePath"
    >

    <canvas id="live2dCanvas" />

    <img
      v-for="key in pressedLeftKeys"
      :key="key"
      :src="resolveKeyImagePath(key)"
    >

    <img
      v-for="key in pressedRightKeys"
      :key="key"
      :src="resolveKeyImagePath(key, 'right')"
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
