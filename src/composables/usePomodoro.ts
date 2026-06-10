import { onUnmounted, watch } from 'vue'
import { usePomodoroStore } from '@/stores/pomodoro'

export function usePomodoro() {
  const pomodoroStore = usePomodoroStore()
  let intervalId: ReturnType<typeof setInterval> | null = null

  const startTimer = () => {
    if (intervalId) return

    intervalId = setInterval(() => {
      pomodoroStore.tick()
    }, 1000)
  }

  const stopTimer = () => {
    if (intervalId) {
      clearInterval(intervalId)
      intervalId = null
    }
  }

  // 监听运行状态
  watch(
    () => pomodoroStore.isRunning,
    (running) => {
      if (running) {
        startTimer()
      } else {
        stopTimer()
      }
    },
    { immediate: true }
  )

  onUnmounted(() => {
    stopTimer()
  })

  return {
    startTimer,
    stopTimer,
  }
}
