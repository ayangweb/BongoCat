import { resolveResource } from '@tauri-apps/api/path'
import { nanoid } from 'nanoid'
import { defineStore } from 'pinia'
import { onMounted, ref } from 'vue'

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

  return {
    models,
    currentModel,
    motions,
    expressions,
  }
})
