<script setup lang="ts">
import type { MotionInfo } from 'easy-live2d'

import { convertFileSrc } from '@tauri-apps/api/core'
import { PhysicalSize } from '@tauri-apps/api/dpi'
import { Menu, PredefinedMenuItem } from '@tauri-apps/api/menu'
import { sep } from '@tauri-apps/api/path'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { exists, readDir } from '@tauri-apps/plugin-fs'
import { useDebounceFn, useEventListener } from '@vueuse/core'
import { round } from 'es-toolkit'
import { nth } from 'es-toolkit/compat'
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'

import { useAppMenu } from '@/composables/useAppMenu'
import { useDevice } from '@/composables/useDevice'
import { useGamepad } from '@/composables/useGamepad'
import { useModel } from '@/composables/useModel'
import { useTauriListen } from '@/composables/useTauriListen'
import { LISTEN_KEY } from '@/constants'
import { hideWindow, setAlwaysOnTop, setTaskbarVisibility, showWindow } from '@/plugins/window'
import { useCatStore } from '@/stores/cat'
import { useGeneralStore } from '@/stores/general.ts'
import { useLabelStore } from '@/stores/label'
import { useModelStore } from '@/stores/model'
import { isImage } from '@/utils/is'
import live2d from '@/utils/live2d'
import { join } from '@/utils/path'
import { isWindows } from '@/utils/platform'
import { clearObject } from '@/utils/shared'

const { startListening } = useDevice()
const appWindow = getCurrentWebviewWindow()
const { modelSize, handleLoad, handleDestroy, handleResize, handleKeyChange } = useModel()
const catStore = useCatStore()
const { getBaseMenu, getExitMenu } = useAppMenu()
const modelStore = useModelStore()
const labelStore = useLabelStore()
const generalStore = useGeneralStore()
const resizing = ref(false)
const backgroundImagePath = ref<string>()
const { stickActive } = useGamepad()
const labelMetrics = computed(() => labelStore.metrics)
const labelReservedHeight = computed(() => labelMetrics.value.reservedHeight)
const labelTitle = computed(() => labelStore.displayTitle.trim())
const labelBody = computed(() => labelStore.displayBodyTag.trim())
const labelTooltip = computed(() => labelStore.tooltipText.trim())
const stageStyle = computed(() => ({
  top: `${labelReservedHeight.value}px`,
  height: `calc(100% - ${labelReservedHeight.value}px)`,
}))
const labelStyle = computed(() => ({
  height: `${labelReservedHeight.value}px`,
  fontSize: `${labelMetrics.value.fontSize}px`,
  lineHeight: `${labelMetrics.value.lineHeight}px`,
}))

onMounted(startListening)

onUnmounted(handleDestroy)

const debouncedResize = useDebounceFn(async () => {
  await handleResize()

  resizing.value = false
}, 100)

useEventListener('resize', () => {
  resizing.value = true

  debouncedResize()
})

watch(() => modelStore.currentModel, async (model) => {
  if (!model) return

  await handleLoad()

  const path = join(model.path, 'resources', 'background.png')

  const existed = await exists(path)

  backgroundImagePath.value = existed ? convertFileSrc(path) : void 0

  clearObject([modelStore.supportKeys, modelStore.pressedKeys])

  const resourcePath = join(model.path, 'resources')
  const groups = ['left-keys', 'right-keys']

  for await (const groupName of groups) {
    const groupDir = join(resourcePath, groupName)
    const files = await readDir(groupDir).catch(() => [])
    const imageFiles = files.filter(file => isImage(file.name))

    for (const file of imageFiles) {
      const fileName = file.name.split('.')[0]

      modelStore.supportKeys[fileName] = join(groupDir, file.name)
    }
  }

  modelStore.modelReady = true
}, { deep: true, immediate: true })

watch([() => catStore.window.scale, modelSize, labelReservedHeight], async ([scale, modelSize]) => {
  if (!modelSize) return

  const { width, height } = modelSize
  const reservedHeight = labelReservedHeight.value

  appWindow.setSize(
    new PhysicalSize({
      width: Math.round(width * (scale / 100)),
      height: Math.round(height * (scale / 100) + reservedHeight),
    }),
  )
}, { immediate: true })

watch([modelStore.pressedKeys, stickActive], ([keys, stickActive]) => {
  const dirs = Object.values(keys).map((path) => {
    return nth(path.split(sep()), -2)!
  })

  const hasLeft = dirs.some(dir => dir.startsWith('left'))
  const hasRight = dirs.some(dir => dir.startsWith('right'))

  handleKeyChange(true, stickActive.left || hasLeft)
  handleKeyChange(false, stickActive.right || hasRight)
}, { deep: true })

watch(() => catStore.window.visible, async (value) => {
  value ? showWindow() : hideWindow()
})

watch(() => catStore.window.passThrough, (value) => {
  appWindow.setIgnoreCursorEvents(value)
}, { immediate: true })

watch(() => catStore.window.alwaysOnTop, setAlwaysOnTop, { immediate: true })

watch(() => generalStore.app.taskbarVisible, setTaskbarVisibility, { immediate: true })

watch(() => catStore.model.motionSound, live2d.setMotionSoundEnabled, { immediate: true })

watch(() => catStore.model.maxFPS, live2d.setMaxFPS, { immediate: true })

useTauriListen<MotionInfo>(LISTEN_KEY.START_MOTION, ({ payload }) => {
  live2d.startMotion(payload)
})

useTauriListen<number>(LISTEN_KEY.SET_EXPRESSION, ({ payload }) => {
  live2d.setExpression(payload)
})

function handleMouseDown() {
  appWindow.startDragging()
}

async function handleContextmenu(event: MouseEvent) {
  event.preventDefault()

  if (event.shiftKey) return

  const menu = await Menu.new({
    items: [
      ...await getBaseMenu(),
      await PredefinedMenuItem.new({ item: 'Separator' }),
      ...await getExitMenu(),
    ],
  })

  // Temporarily disable always-on-top on Windows so the context menu is not covered
  if (isWindows && catStore.window.alwaysOnTop) {
    setAlwaysOnTop(false)
  }

  await menu.popup()

  // Restore always-on-top after the menu is closed
  if (!isWindows || !catStore.window.alwaysOnTop) return

  setAlwaysOnTop(true)
}

function handleMouseMove(event: MouseEvent) {
  const { buttons, shiftKey, movementX, movementY } = event

  if (buttons !== 2 || !shiftKey) return

  const delta = (movementX + movementY) * 0.5
  const nextScale = Math.max(10, Math.min(catStore.window.scale + delta, 500))

  catStore.window.scale = round(nextScale)
}
</script>

<template>
  <div
    class="relative size-screen overflow-hidden"
    :style="{
      opacity: catStore.window.opacity / 100,
      borderRadius: `${catStore.window.radius}%`,
    }"
    @contextmenu="handleContextmenu"
    @mousedown="handleMouseDown"
    @mousemove="handleMouseMove"
  >
    <div
      v-if="labelMetrics.isVisible"
      class="pointer-events-none absolute inset-x-0 top-0 z-20 flex items-center justify-center rounded-xl bg-[rgba(0,0,0,0.45)] px-4 text-center text-white font-bold"
      :class="{ 'opacity-50': labelStore.runtimeState === 'dormant' }"
      :style="labelStyle"
    >
      <span
        class="line-clamp-2 w-full break-all text-center leading-tight"
        :title="labelTooltip"
      >
        <span
          v-if="labelTitle"
          class="mr-0.5 opacity-80"
        >{{ labelTitle }}</span><span>{{ labelBody }}</span>
      </span>
    </div>

    <div
      id="modelStage"
      class="absolute inset-x-0 bottom-0 overflow-hidden children:(absolute size-full)"
      :class="{ '-scale-x-100': catStore.model.mirror }"
      :style="stageStyle"
    >
      <img
        v-if="backgroundImagePath"
        class="object-cover"
        :src="backgroundImagePath"
      >

      <canvas id="live2dCanvas" />

      <img
        v-for="path in modelStore.pressedKeys"
        :key="path"
        class="object-cover"
        :src="convertFileSrc(path)"
      >
    </div>

    <div
      v-show="resizing || !modelStore.modelReady"
      class="absolute inset-0 z-30 flex items-center justify-center bg-black"
    >
      <span class="text-center text-[10vw] text-white">
        {{ resizing ? $t('pages.main.hints.redrawing') : $t('pages.main.hints.switching') }}
      </span>
    </div>
  </div>
</template>
