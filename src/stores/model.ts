import { resolveResource } from '@tauri-apps/api/path'
import { nanoid } from 'nanoid'
import { defineStore } from 'pinia'
import { ref } from 'vue'

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

  const init = async () => {
    const presetModelsPath = await resolveResource('assets/models')

    if (models.value.length === 0) {
      const modes: ModelMode[] = ['standard', 'keyboard']

      for await (const mode of modes) {
        models.value.push({
          mode,
          id: nanoid(),
          isPreset: true,
          path: join(presetModelsPath, mode),
        })
      }
    } else {
      models.value = models.value.map((item) => {
        const { isPreset, mode } = item

        if (!isPreset) return item

        return {
          ...item,
          path: join(presetModelsPath, mode),
        }
      })
    }

    const matched = models.value.find(item => item.id === currentModel.value?.id)

    if (matched) {
      return currentModel.value = matched
    }

    currentModel.value = models.value[0]
  }

  return {
    models,
    currentModel,
    motions,
    expressions,
    init,
  }
})
