import { resolveResource } from '@tauri-apps/api/path'
import { filter, find } from 'es-toolkit/compat'
import { nanoid } from 'nanoid'
import { defineStore } from 'pinia'
import { reactive, ref } from 'vue'

import { join } from '@/utils/path'

export type ModelMode = 'standard' | 'keyboard' | 'gamepad'

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
  const supportKeys = reactive<Record<string, string>>({})
  const pressedKeys = reactive<Record<string, string>>({})

  const init = async () => {
    const modelsPath = await resolveResource('assets/models')

    const nextModels = filter(models.value, { isPreset: false })
    const presetModels = filter(models.value, { isPreset: true })

    const modes: ModelMode[] = ['gamepad', 'keyboard', 'standard']

    for (const mode of modes) {
      const matched = find(presetModels, { mode })

      nextModels.unshift({
        id: matched?.id ?? nanoid(),
        mode,
        isPreset: true,
        path: join(modelsPath, mode),
      })
    }

    const matched = find(nextModels, { id: currentModel.value?.id })

    currentModel.value = matched ?? nextModels[0]

    models.value = nextModels
  }

  return {
    models,
    currentModel,
    motions,
    expressions,
    supportKeys,
    pressedKeys,
    init,
  }
}, {
  tauri: {
    filterKeys: ['models', 'currentModel'],
    filterKeysStrategy: 'pick',
  },
})
