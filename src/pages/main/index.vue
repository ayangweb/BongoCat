<script setup lang="ts">
import type { MotionInfo } from 'easy-live2d'

import { convertFileSrc } from '@tauri-apps/api/core'
import { Menu, PredefinedMenuItem } from '@tauri-apps/api/menu'
import { sep } from '@tauri-apps/api/path'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { exists, readDir } from '@tauri-apps/plugin-fs'
import { useDebounceFn, useEventListener } from '@vueuse/core'
import { round } from 'es-toolkit'
import { nth } from 'es-toolkit/compat'
import { nextTick, onMounted, onUnmounted, ref, watch } from 'vue'

import { useAppMenu } from '@/composables/useAppMenu'
import { useDevice } from '@/composables/useDevice'
import { useGamepad } from '@/composables/useGamepad'
import { useModel } from '@/composables/useModel'
import { useTauriListen } from '@/composables/useTauriListen'
import { LISTEN_KEY } from '@/constants'
import { hideWindow, setAlwaysOnTop, setTaskbarVisibility, showWindow } from '@/plugins/window'
import { useCatStore } from '@/stores/cat'
import { useGeneralStore } from '@/stores/general.ts'
import { useModelStore } from '@/stores/model'
import { isImage } from '@/utils/is'
import live2d from '@/utils/live2d'
import { join } from '@/utils/path'
import { isWindows } from '@/utils/platform'
import { clearObject } from '@/utils/shared'

const { startListening } = useDevice()
const appWindow = getCurrentWebviewWindow()
const { modelSize, handleLoad, handleDestroy, handleResize, syncWindowSize, handleKeyChange } = useModel()
const catStore = useCatStore()
const { getBaseMenu, getExitMenu } = useAppMenu()
const modelStore = useModelStore()
const generalStore = useGeneralStore()
const resizing = ref(false)
const backgroundImagePath = ref<string>()
const { stickActive } = useGamepad()
const isCanvasReady = ref(false)
const INITIAL_MODEL_LOAD_RETRY_DELAY_MS = 100
let loadedModelId: string | undefined
let modelLoadVersion = 0
let modelLoadBarrier = Promise.resolve()

onUnmounted(handleDestroy)

const debouncedResize = useDebounceFn(async () => {
  await handleResize()

  resizing.value = false
}, 100)

useEventListener('resize', () => {
  resizing.value = true

  debouncedResize()
})

function waitForAnimationFrame() {
  return new Promise<void>((resolve) => {
    requestAnimationFrame(() => resolve())
  })
}

function waitForDelay(delayMs: number) {
  return new Promise<void>((resolve) => {
    setTimeout(resolve, delayMs)
  })
}

async function ensureCanvasReady() {
  await nextTick()

  let canvas = document.getElementById('live2dCanvas')

  if (!(canvas instanceof HTMLCanvasElement)) {
    await waitForAnimationFrame()
    canvas = document.getElementById('live2dCanvas')
  }

  if (!(canvas instanceof HTMLCanvasElement)) {
    throw new TypeError('[main] #live2dCanvas is not ready')
  }

  isCanvasReady.value = true
}

async function loadCurrentModel(
  model = modelStore.currentModel,
  options: { retryOnFailure?: boolean } = {},
) {
  if (!model) return false

  const version = ++modelLoadVersion
  let completed = false
  const previousLoad = modelLoadBarrier
  let releaseCurrentLoad!: () => void
  const currentLoad = new Promise<void>((resolve) => {
    releaseCurrentLoad = resolve
  })

  modelLoadBarrier = currentLoad

  await previousLoad

  const isActiveLoad = () => {
    return version === modelLoadVersion && modelStore.currentModel?.id === model.id
  }

  try {
    if (!isActiveLoad()) {
      return false
    }

    modelStore.modelReady = false

    let didLoad = await handleLoad(model, { showError: !options.retryOnFailure })

    if (!didLoad && options.retryOnFailure) {
      await waitForDelay(INITIAL_MODEL_LOAD_RETRY_DELAY_MS)

      if (!isActiveLoad()) {
        return false
      }

      didLoad = await handleLoad(model)
    }

    if (!didLoad || !isActiveLoad()) {
      return false
    }

    const didSyncWindowSize = await syncWindowSize()

    if (!didSyncWindowSize) {
      return false
    }

    if (!isActiveLoad()) {
      return false
    }

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

    if (!isActiveLoad()) {
      return false
    }

    loadedModelId = model.id
    modelStore.modelReady = true
    completed = true

    return true
  } finally {
    if (!completed && isActiveLoad()) {
      modelStore.modelReady = true
    }

    releaseCurrentLoad()
  }
}

onMounted(async () => {
  startListening()
  await ensureCanvasReady()
  await loadCurrentModel(modelStore.currentModel, { retryOnFailure: true })
})

watch(() => modelStore.currentModel?.id, async (modelId) => {
  if (!isCanvasReady.value || !modelId || modelId === loadedModelId) return

  await loadCurrentModel()
})

watch(() => catStore.window.scale, async () => {
  if (!modelSize.value) return

  await syncWindowSize()
})

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
    class="relative size-screen overflow-hidden children:(absolute size-full)"
    :class="{ '-scale-x-100': catStore.model.mirror }"
    :style="{
      opacity: catStore.window.opacity / 100,
      borderRadius: `${catStore.window.radius}%`,
    }"
    @contextmenu="handleContextmenu"
    @mousedown="handleMouseDown"
    @mousemove="handleMouseMove"
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

    <div
      v-show="resizing || !modelStore.modelReady"
      class="flex items-center justify-center bg-black"
    >
      <span class="text-center text-[10vw] text-[#fff]">
        {{ resizing ? $t('pages.main.hints.redrawing') : $t('pages.main.hints.switching') }}
      </span>
    </div>
  </div>
</template>
