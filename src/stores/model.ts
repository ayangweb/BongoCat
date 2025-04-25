import { defineStore } from 'pinia'
import { ref } from 'vue'

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
  const motions = ref<MotionGroup>({})
  const expressions = ref<Expression[]>([])

  return {
    motions,
    expressions,
  }
})
