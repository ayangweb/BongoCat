<script setup lang="ts">
import { LogicalSize, PhysicalPosition } from '@tauri-apps/api/dpi'
import { TauriEvent } from '@tauri-apps/api/event'
import { getCurrentWebviewWindow, WebviewWindow } from '@tauri-apps/api/webviewWindow'
import { availableMonitors } from '@tauri-apps/api/window'
import { computed, nextTick, onMounted, onUnmounted, reactive, ref, watch } from 'vue'

import { useTauriListen } from '@/composables/useTauriListen'
import { LISTEN_KEY, WINDOW_LABEL } from '@/constants'
import { useAiStore } from '@/stores/ai'
import { computeBubblePosition } from '@/utils/chatPosition'
import { isMac } from '@/utils/platform'

interface ShowChatPayload {
  text: string
  duration?: number
  textColor?: string
  fontSize?: number
  bgColor?: string
  bgOpacity?: number
}

const GAP = 8 // 气泡与猫的间距（逻辑像素），定位时 × scaleFactor 转物理

const appWindow = getCurrentWebviewWindow()
const aiStore = useAiStore()
const bubbleRef = ref<HTMLElement>()
const text = ref('')
const visible = ref(false)

// 本条气泡的一次性样式覆盖；每次 showChat 重置，不写入 aiStore（设置不变）
const override = reactive<{
  textColor?: string
  fontSize?: number
  bgColor?: string
  bgOpacity?: number
}>({})

let timer: ReturnType<typeof setTimeout> | undefined
const unlisteners: Array<() => void> = []

// 气泡背景：hex + 透明度(0-100) 合成 rgba；窗口本身始终透明
function hexToRgba(hex: string, opacity: number) {
  const value = hex.replace('#', '')
  const full = value.length === 3 ? value.split('').map(c => c + c).join('') : value
  if (full.length !== 6) return `rgba(0, 0, 0, ${opacity / 100})`
  const r = Number.parseInt(full.slice(0, 2), 16)
  const g = Number.parseInt(full.slice(2, 4), 16)
  const b = Number.parseInt(full.slice(4, 6), 16)
  return `rgba(${r}, ${g}, ${b}, ${opacity / 100})`
}

const bgRgba = computed(() => hexToRgba(override.bgColor ?? aiStore.ai.bgColor, override.bgOpacity ?? aiStore.ai.bgOpacity))

const bubbleStyle = computed(() => ({
  color: override.textColor ?? aiStore.ai.textColor,
  fontSize: `${override.fontSize ?? aiStore.ai.fontSize}px`,
  background: bgRgba.value,
}))

const triangleStyle = computed(() => ({
  borderTopColor: bgRgba.value,
}))

async function resize() {
  await nextTick()

  const el = bubbleRef.value
  if (!el) return

  // bubbleRef 是带 padding 的外层 wrapper（padding 给阴影留白，且会被 getBoundingClientRect 计入）；
  // wrapper 是普通 block + w-max，宽度由内容决定、与当前窗口宽度无关，不会被 flex 压缩成竖排。
  const rect = el.getBoundingClientRect()

  await appWindow.setSize(new LogicalSize(Math.ceil(rect.width), Math.ceil(rect.height)))
}

async function reposition() {
  const el = bubbleRef.value
  if (!visible.value || !el) return

  const main = await WebviewWindow.getByLabel(WINDOW_LABEL.MAIN)
  if (!main) return

  const [position, size, monitors] = await Promise.all([
    main.outerPosition(),
    main.outerSize(),
    availableMonitors(),
  ])

  // 猫所在显示器：包含猫中心点的那块屏（全物理像素比较，多屏/不同 DPI 都正确）
  const centerX = position.x + size.width / 2
  const centerY = position.y + size.height / 2
  const monitor = monitors.find(({ position: p, size: s }) => {
    return centerX >= p.x && centerX < p.x + s.width && centerY >= p.y && centerY < p.y + s.height
  }) ?? monitors[0]

  if (!monitor) return

  const sf = monitor.scaleFactor
  const rect = el.getBoundingClientRect()

  const { x, y } = computeBubblePosition(
    { x: position.x, y: position.y, w: size.width, h: size.height },
    { w: rect.width * sf, h: rect.height * sf },
    { x: monitor.position.x, y: monitor.position.y, w: monitor.size.width, h: monitor.size.height },
    GAP * sf,
  )

  await appWindow.setPosition(new PhysicalPosition(Math.round(x), Math.round(y)))
}

function hide() {
  visible.value = false // 触发淡出；@after-leave 里再 appWindow.hide()
}

async function showChat({ text: nextText, duration, textColor, fontSize, bgColor, bgOpacity }: ShowChatPayload) {
  // 总开关唯一生效点
  if (!aiStore.ai.enabled) return

  // 一次性覆盖：赋 undefined 即回落到 store 默认
  override.textColor = textColor
  override.fontSize = fontSize
  override.bgColor = bgColor
  override.bgOpacity = bgOpacity

  text.value = nextText
  visible.value = true

  await resize()
  await reposition()
  await appWindow.show()

  // 默认时长唯一兜底点；0 表示常驻
  const ms = duration ?? aiStore.ai.duration * 1000

  clearTimeout(timer)

  if (ms > 0) {
    timer = setTimeout(hide, ms)
  }
}

onMounted(async () => {
  await appWindow.setIgnoreCursorEvents(true)

  if (isMac) {
    // macOS：NSPanel 不触发原生 move/resize，由 macos.rs 广播 tauri://move / tauri://resize
    unlisteners.push(await appWindow.listen(TauriEvent.WINDOW_MOVED, reposition))
    unlisteners.push(await appWindow.listen(TauriEvent.WINDOW_RESIZED, reposition))
  } else {
    // Windows/Linux：原生监听主窗口几何变化
    const main = await WebviewWindow.getByLabel(WINDOW_LABEL.MAIN)
    if (main) {
      unlisteners.push(await main.onMoved(reposition))
      unlisteners.push(await main.onResized(reposition))
    }
  }
})

// ponytail: singleton overlay window won't remount in prod, but cleanup is cheap and correct
onUnmounted(() => {
  unlisteners.forEach(fn => fn())
  clearTimeout(timer)
})

useTauriListen<ShowChatPayload>(LISTEN_KEY.SHOW_CHAT, ({ payload }) => {
  showChat(payload)
})

interface UpdateConfigPayload {
  duration?: number
  textColor?: string
  fontSize?: number
  bgColor?: string
  bgOpacity?: number
}

// 控制接口：写入 aiStore 默认值，saveOnChange 落盘并跨窗口同步到设置页
useTauriListen<UpdateConfigPayload>(LISTEN_KEY.UPDATE_CONFIG, ({ payload }) => {
  const { duration, textColor, fontSize, bgColor, bgOpacity } = payload

  if (duration !== undefined) aiStore.ai.duration = duration
  if (textColor !== undefined) aiStore.ai.textColor = textColor
  if (fontSize !== undefined) aiStore.ai.fontSize = fontSize
  if (bgColor !== undefined) aiStore.ai.bgColor = bgColor
  if (bgOpacity !== undefined) aiStore.ai.bgOpacity = bgOpacity
})

// 字号改变会改变气泡尺寸：可见时重新测量并定位
watch(() => aiStore.ai.fontSize, async () => {
  if (!visible.value) return
  await resize()
  await reposition()
})
</script>

<template>
  <div class="size-screen overflow-hidden">
    <Transition
      name="fade"
      @after-leave="appWindow.hide()"
    >
      <!-- wrapper：w-max 让宽度跟随内容（横向排版），max-w-80 到达上限后才换行；p-3 给阴影留白并被测量计入 -->
      <div
        v-show="visible"
        ref="bubbleRef"
        class="max-w-80 w-max p-3"
      >
        <div
          class="relative whitespace-pre-wrap break-words px-3 py-2 leading-relaxed rounded-2xl shadow-lg"
          :style="bubbleStyle"
        >
          {{ text }}

          <span
            class="absolute left-1/2 top-full h-0 w-0 b-6 b-solid b-transparent -translate-x-1/2"
            :style="triangleStyle"
          />
        </div>
      </div>
    </Transition>
  </div>
</template>

<style scoped>
.fade-enter-active,
.fade-leave-active {
  transition: opacity 0.2s ease;
}

.fade-enter-from,
.fade-leave-to {
  opacity: 0;
}
</style>
