<script setup lang="ts">
import { LogicalSize, PhysicalPosition } from '@tauri-apps/api/dpi'
import { TauriEvent } from '@tauri-apps/api/event'
import { getCurrentWebviewWindow, WebviewWindow } from '@tauri-apps/api/webviewWindow'
import { availableMonitors } from '@tauri-apps/api/window'
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'

import { useTauriListen } from '@/composables/useTauriListen'
import { LISTEN_KEY, WINDOW_LABEL } from '@/constants'
import { useAiStore } from '@/stores/ai'
import { computeBubblePosition } from '@/utils/chatPosition'
import { isMac } from '@/utils/platform'

interface ShowChatPayload {
  text: string
  duration?: number
}

const GAP = 8 // 气泡与猫的间距（逻辑像素），定位时 × scaleFactor 转物理

const appWindow = getCurrentWebviewWindow()
const aiStore = useAiStore()
const bubbleRef = ref<HTMLElement>()
const text = ref('')
const visible = ref(false)

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

const bgRgba = computed(() => hexToRgba(aiStore.ai.bgColor, aiStore.ai.bgOpacity))

const bubbleStyle = computed(() => ({
  color: aiStore.ai.textColor,
  fontSize: `${aiStore.ai.fontSize}px`,
  background: bgRgba.value,
}))

const triangleStyle = computed(() => ({
  borderTopColor: bgRgba.value,
}))

async function resize() {
  await nextTick()

  const el = bubbleRef.value
  if (!el) return

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

async function showChat({ text: nextText, duration }: ShowChatPayload) {
  // 总开关唯一生效点
  if (!aiStore.ai.enabled) return

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

// 字号改变会改变气泡尺寸：可见时重新测量并定位
watch(() => aiStore.ai.fontSize, async () => {
  if (!visible.value) return
  await resize()
  await reposition()
})
</script>

<template>
  <div class="size-screen flex items-end justify-center overflow-hidden">
    <Transition
      name="fade"
      @after-leave="appWindow.hide()"
    >
      <div
        v-show="visible"
        ref="bubbleRef"
        class="relative m-3 max-w-80 w-max whitespace-pre-wrap break-words px-3 py-2 leading-relaxed rounded-2xl shadow-lg"
        :style="bubbleStyle"
      >
        {{ text }}

        <span
          class="absolute left-1/2 top-full h-0 w-0 b-6 b-solid b-transparent -translate-x-1/2"
          :style="triangleStyle"
        />
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
