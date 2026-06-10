import { defineStore } from 'pinia'
import { ref, computed } from 'vue'

export type PomodoroPhase = 'work' | 'shortBreak' | 'longBreak'

export interface PomodoroSettings {
  workDuration: number        // 工作时长(分钟)，默认25
  shortBreakDuration: number // 短休息时长(分钟)，默认5
  longBreakDuration: number  // 长休息时长(分钟)，默认15
  longBreakInterval: number  // 长休息间隔(番茄数)，默认4
  autoStartBreak: boolean    // 自动开始休息
  autoStartWork: boolean     // 自动开始工作
  enabled: boolean           // 功能开关
}

export interface PomodoroState {
  isRunning: boolean
  currentPhase: PomodoroPhase
  timeRemaining: number       // 剩余秒数
  sessionsCompleted: number   // 完成的番茄数
  currentSession: number      // 当前是第几个番茄(1-4)
}

export const usePomodoroStore = defineStore('pomodoro', () => {
  // 番茄时钟功能开关
  const enabled = ref(false)

  // 番茄时钟配置
  const settings = ref<PomodoroSettings>({
    workDuration: 25,
    shortBreakDuration: 5,
    longBreakDuration: 15,
    longBreakInterval: 4,
    autoStartBreak: false,
    autoStartWork: false,
  })

  // 状态
  const isRunning = ref(false)
  const currentPhase = ref<PomodoroPhase>('work')
  const timeRemaining = ref(settings.value.workDuration * 60) // 秒
  const sessionsCompleted = ref(0)
  const currentSession = ref(1)

  // 计算属性
  const totalTime = computed(() => {
    switch (currentPhase.value) {
      case 'work':
        return settings.value.workDuration * 60
      case 'shortBreak':
        return settings.value.shortBreakDuration * 60
      case 'longBreak':
        return settings.value.longBreakDuration * 60
    }
  })

  const progress = computed(() => {
    if (totalTime.value === 0) return 0
    return ((totalTime.value - timeRemaining.value) / totalTime.value) * 100
  })

  const formattedTime = computed(() => {
    const minutes = Math.floor(timeRemaining.value / 60)
    const seconds = timeRemaining.value % 60
    return `${String(minutes).padStart(2, '0')}:${String(seconds).padStart(2, '0')}`
  })

  // 重置时间
  const resetTime = () => {
    timeRemaining.value = totalTime.value
  }

  // 开始计时
  const start = () => {
    isRunning.value = true
  }

  // 暂停计时
  const pause = () => {
    isRunning.value = false
  }

  // 切换计时
  const toggle = () => {
    isRunning.value = !isRunning.value
  }

  // 跳到下一阶段
  const nextPhase = () => {
    isRunning.value = false

    if (currentPhase.value === 'work') {
      sessionsCompleted.value++
      currentSession.value++

      if (currentSession.value > settings.value.longBreakInterval) {
        currentPhase.value = 'longBreak'
        currentSession.value = 1
      } else {
        currentPhase.value = 'shortBreak'
      }

      if (settings.value.autoStartBreak) {
        start()
      }
    } else {
      currentPhase.value = 'work'

      if (settings.value.autoStartWork) {
        start()
      }
    }

    resetTime()
  }

  // 重置所有状态
  const reset = () => {
    isRunning.value = false
    currentPhase.value = 'work'
    timeRemaining.value = settings.value.workDuration * 60
    sessionsCompleted.value = 0
    currentSession.value = 1
  }

  // tick (每秒调用)
  const tick = () => {
    if (!isRunning.value || timeRemaining.value <= 0) return

    timeRemaining.value--

    if (timeRemaining.value <= 0) {
      nextPhase()
    }
  }

  return {
    enabled,
    settings,
    isRunning,
    currentPhase,
    timeRemaining,
    sessionsCompleted,
    currentSession,
    totalTime,
    progress,
    formattedTime,
    resetTime,
    start,
    pause,
    toggle,
    nextPhase,
    reset,
    tick,
  }
})
