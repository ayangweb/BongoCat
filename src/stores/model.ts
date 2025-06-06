import { resolveResource } from '@tauri-apps/api/path'
import { nanoid } from 'nanoid'
import { defineStore } from 'pinia'
import { onMounted, ref, watch } from 'vue'

import { join } from '@/utils/path'

export type ModelMode = 'standard' | 'keyboard' | 'handle'

export interface Model {
  id: string
  path: string
  mode: ModelMode
  isPreset: boolean
}

interface Motion {
  Name: string
  File: string
  Sound?: string
  FadeInTime: number
  FadeOutTime: number
  Description?: string
}

type MotionGroup = Record<string, Motion[]>

interface Expression {
  Name: string
  File: string
  Description?: string
}

export const useModelStore = defineStore('model', () => {
  const models = ref<Model[]>([])
  const currentModel = ref<Model>()
  const motions = ref<MotionGroup>({})
  const expressions = ref<Expression[]>([])
  const autoSwitchEnabled = ref(false)
  const autoSwitchInterval = ref(5) // 默认5秒
  let autoSwitchTimer: NodeJS.Timeout | null = null

  onMounted(async () => {
    const modelsPath = await resolveResource('assets/models')

    if (models.value.length === 0) {
      const modes: ModelMode[] = ['standard', 'keyboard']

      for await (const mode of modes) {
        const path = join(modelsPath, mode)

        models.value.push({
          id: nanoid(),
          path,
          mode,
          isPreset: true,
        })
      }
    }

    if (currentModel.value) return

    currentModel.value = models.value[0]
  })

  // 监听自动切换开关状态
  watch(autoSwitchEnabled, (enabled) => {
    if (enabled) {
      startAutoSwitch()
    } else {
      stopAutoSwitch()
    }
  })

  function startAutoSwitch() {
    if (!autoSwitchEnabled.value || models.value.length <= 1) return

    stopAutoSwitch()

    autoSwitchTimer = setInterval(() => {
      const currentIndex = models.value.findIndex(model => model.id === currentModel.value?.id)
      const nextIndex = (currentIndex + 1) % models.value.length
      currentModel.value = models.value[nextIndex]
    }, autoSwitchInterval.value * 1000)
  }

  function stopAutoSwitch() {
    if (autoSwitchTimer) {
      clearInterval(autoSwitchTimer)
      autoSwitchTimer = null
    }
  }

  function resetAutoSwitchTimer() {
    if (!autoSwitchEnabled.value) return

    stopAutoSwitch()
    startAutoSwitch()
  }

  return {
    models,
    currentModel,
    motions,
    expressions,
    autoSwitchEnabled,
    autoSwitchInterval,
    startAutoSwitch,
    stopAutoSwitch,
    resetAutoSwitchTimer,
  }
})
