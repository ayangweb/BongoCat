import type { ExpressionInfo, MotionInfo } from 'easy-live2d'

import { resolveResource } from '@tauri-apps/api/path'
import { readDir, readTextFile } from '@tauri-apps/plugin-fs'
import { filter } from 'es-toolkit/compat'
import { defineStore } from 'pinia'
import { reactive, ref } from 'vue'

import { join } from '@/utils/path'

export type ModelMode = 'standard' | 'keyboard' | 'gamepad'
export type ModelRenderer = 'live2d' | 'sprite'

export interface Model {
  id: string
  path: string
  mode: ModelMode
  renderer: ModelRenderer
  displayName?: string
  isPreset: boolean
}

interface SpriteModelManifest {
  id?: unknown
  displayName?: unknown
  mode?: unknown
  renderer?: unknown
}

export const useModelStore = defineStore('model', () => {
  const modelReady = ref(true)
  const models = ref<Model[]>([])
  const currentModel = ref<Model>()
  const supportKeys = reactive<Record<string, string>>({})
  const pressedKeys = reactive<Record<string, string>>({})
  const currentMotions = ref<Array<[string, MotionInfo[]]>>([])
  const currentExpressions = ref<ExpressionInfo[]>([])
  const shortcuts = reactive<Record<string, string>>({})

  const init = async () => {
    const modelsPath = await resolveResource('assets/models')
    const previousModels = models.value.map(model => ({
      ...model,
      renderer: model.renderer ?? ('live2d' as const),
    }))
    const previousCurrent = currentModel.value
      ? {
          ...currentModel.value,
          renderer: currentModel.value.renderer ?? ('live2d' as const),
        }
      : void 0

    const customModels = filter(previousModels, { isPreset: false })
    const previousPresetModels = filter(previousModels, { isPreset: true })
    const modes: ModelMode[] = ['gamepad', 'keyboard', 'standard']

    const spriteModels: Model[] = []
    const spriteIds = new Set<string>()
    const entries = await readDir(modelsPath).catch(() => [])

    for (const entry of entries.sort((left, right) => left.name.localeCompare(right.name))) {
      if (!entry.isDirectory || modes.includes(entry.name as ModelMode)) continue

      const path = join(modelsPath, entry.name)

      try {
        const manifest = JSON.parse(
          await readTextFile(join(path, 'model.json')),
        ) as SpriteModelManifest

        if (manifest.renderer !== 'sprite') continue

        const mode = modes.includes(manifest.mode as ModelMode)
          ? manifest.mode as ModelMode
          : 'keyboard'
        const manifestId = typeof manifest.id === 'string' && manifest.id.trim()
          ? manifest.id.trim()
          : entry.name

        if (spriteIds.has(manifestId)) continue

        spriteIds.add(manifestId)

        spriteModels.push({
          id: `preset-sprite-${manifestId}`,
          mode,
          renderer: 'sprite',
          displayName: typeof manifest.displayName === 'string'
            ? manifest.displayName
            : entry.name,
          isPreset: true,
          path,
        })
      } catch {
        continue
      }
    }

    const live2dModels = modes.slice().reverse().map<Model>(mode => ({
      id: `preset-live2d-${mode}`,
      mode,
      renderer: 'live2d',
      isPreset: true,
      path: join(modelsPath, mode),
    }))
    const nextModels = [...spriteModels, ...live2dModels, ...customModels]

    for (const previous of previousPresetModels) {
      if (previous.renderer !== 'live2d') continue

      const next = live2dModels.find(model => model.mode === previous.mode)

      if (!next || previous.id === next.id) continue

      const prefix = `${previous.id}:`

      for (const [key, shortcut] of Object.entries(shortcuts)) {
        if (!key.startsWith(prefix)) continue

        const nextKey = `${next.id}:${key.slice(prefix.length)}`

        if (!shortcuts[nextKey]) shortcuts[nextKey] = shortcut

        delete shortcuts[key]
      }
    }

    let matched: Model | undefined

    if (previousCurrent?.isPreset) {
      if (previousCurrent.renderer === 'sprite') {
        matched = spriteModels.find(model => model.path === previousCurrent.path)

        if (!matched) {
          const legacyId = previousCurrent.id.replace(/^preset-(?:sprite-)?/, '')

          matched = spriteModels.find(model => model.id === `preset-sprite-${legacyId}`)
        }
      } else {
        matched = live2dModels.find(model => model.mode === previousCurrent.mode)
      }
    } else if (previousCurrent) {
      matched = customModels.find(model => model.id === previousCurrent.id)
    }

    currentModel.value = matched ?? nextModels[0]

    models.value = nextModels
  }

  return {
    modelReady,
    models,
    currentModel,
    supportKeys,
    pressedKeys,
    currentMotions,
    currentExpressions,
    shortcuts,
    init,
  }
}, {
  tauri: {
    filterKeys: ['supportKeys', 'pressedKeys'],
  },
})
