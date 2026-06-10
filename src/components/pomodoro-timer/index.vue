<script setup lang="ts">
import { usePomodoroStore } from '@/stores/pomodoro'

const pomodoroStore = usePomodoroStore()

const phaseLabels: Record<string, string> = {
  work: '🍅',
  shortBreak: '☕',
  longBreak: '🌴',
}
</script>

<template>
  <div
    v-if="pomodoroStore.enabled"
    class="pomodoro-timer"
  >
    <div class="timer-display">
      <span class="phase-icon">{{ phaseLabels[pomodoroStore.currentPhase] }}</span>
      <span class="time">{{ pomodoroStore.formattedTime }}</span>
    </div>

    <div class="timer-controls">
      <button
        class="control-btn"
        @click="pomodoroStore.toggle"
      >
        {{ pomodoroStore.isRunning ? '⏸' : '▶' }}
      </button>
      <button
        class="control-btn"
        @click="pomodoroStore.nextPhase"
      >
        ⏭
      </button>
    </div>

    <div class="session-info">
      <span
        v-for="i in pomodoroStore.settings.longBreakInterval"
        :key="i"
        class="session-dot"
        :class="{ completed: i <= pomodoroStore.currentSession - 1 }"
      >
        🍅
      </span>
    </div>
  </div>
</template>

<style scoped>
.pomodoro-timer {
  position: fixed;
  bottom: 20px;
  right: 20px;
  background: rgba(0, 0, 0, 0.85);
  color: white;
  padding: 12px 16px;
  border-radius: 12px;
  font-family: monospace;
  z-index: 1000;
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-width: 120px;
  backdrop-filter: blur(8px);
  border: 1px solid rgba(255, 255, 255, 0.1);
}

.timer-display {
  display: flex;
  align-items: center;
  gap: 8px;
  justify-content: center;
}

.phase-icon {
  font-size: 20px;
}

.time {
  font-size: 24px;
  font-weight: bold;
  font-variant-numeric: tabular-nums;
}

.timer-controls {
  display: flex;
  gap: 8px;
  justify-content: center;
}

.control-btn {
  background: rgba(255, 255, 255, 0.15);
  border: 1px solid rgba(255, 255, 255, 0.2);
  color: white;
  padding: 4px 12px;
  border-radius: 6px;
  cursor: pointer;
  transition: background 0.2s;
}

.control-btn:hover {
  background: rgba(255, 255, 255, 0.25);
}

.session-info {
  display: flex;
  gap: 4px;
  justify-content: center;
}

.session-dot {
  font-size: 12px;
  opacity: 0.3;
  transition: opacity 0.3s;
}

.session-dot.completed {
  opacity: 1;
}
</style>
