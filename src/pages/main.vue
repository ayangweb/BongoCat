<script setup lang="ts">
import { LogicalSize } from '@tauri-apps/api/dpi'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { Live2DModel } from 'pixi-live2d-display'
import { Ticker } from 'pixi.js'
import { onMounted, ref, watch } from 'vue'

import { useDevice } from '../composables/useDevice'
import { useModel } from '../composables/useModel'
import { getCursorMonitor } from '../utils/monitor'

Live2DModel.registerTicker(Ticker)

const mode = ref('standard')
const { pressedKeys, pressedMouses, mousePosition } = useDevice()
const { model, setParameterValue } = useModel()

onMounted(() => {
  window.addEventListener('resize', handleResized)
})

watch(model, (value) => {
  if (!value) return

  handleResized()
})

watch(pressedKeys, (value) => {
  const hasArrowKey = value.some(key => key.endsWith('Arrow'))
  const hasNonArrowKey = value.some(key => !key.endsWith('Arrow'))

  setParameterValue('CatParamRightHandDown', hasArrowKey)
  setParameterValue('CatParamLeftHandDown', hasNonArrowKey)
})

watch(pressedMouses, (value) => {
  const isLeftDown = value.includes('Left')
  const isRightDown = value.includes('Right')

  setParameterValue('ParamMouseLeftDown', isLeftDown)
  setParameterValue('ParamMouseRightDown', isRightDown)
})

watch(mousePosition, async () => {
  if (!model.value) return

  const monitor = await getCursorMonitor()

  if (!monitor) return

  const { size, cursorX, cursorY } = monitor
  const { width, height } = size

  const xRatio = cursorX / width
  const yRatio = cursorY / height

  const x = (xRatio * 60) - 30
  const y = (yRatio * 60) - 30

  setParameterValue('ParamMouseX', -x)
  setParameterValue('ParamMouseY', -y)
  setParameterValue('ParamAngleX', x)
  setParameterValue('ParamAngleY', -y)
})

async function handleResized() {
  if (!model.value) return

  const { innerWidth } = window

  await getCurrentWebviewWindow().setSize(
    new LogicalSize({
      width: innerWidth,
      height: innerWidth * (354 / 612),
    }),
  )

  model.value.scale.set(innerWidth / 612)
}

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
    <img :src="`/images/backgrounds/${mode}.png`">

    <canvas id="live2dCanvas" />

    <template v-for="key in pressedKeys" :key="key">
      <img :src="`/images/keys/${key}.png`">
      <img :src="`/images/hands/${key}.png`">
    </template>
  </div>
</template>
