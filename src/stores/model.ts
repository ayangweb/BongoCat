import { defineStore } from 'pinia'
import { ref } from 'vue'

interface Motion {
  Key: string
  Name: string
  File: string
  Sound?: string
  FadeInTime: number
  FadeOutTime: number
}

interface Expression {
  Key: string
  Name: string
  File: string
}

export const useModelStore = defineStore('model', () => {
  const motions = ref<Motion[]>([])
  const expressions = ref<Expression[]>([])

  return {
    motions,
    expressions,
  }
})
