<script setup lang="ts">
import { invoke } from '@tauri-apps/api/core'
import { appDataDir } from '@tauri-apps/api/path'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { open } from '@tauri-apps/plugin-dialog'
import { exists, readDir, readTextFile } from '@tauri-apps/plugin-fs'
import { message } from 'antdv-next'
import { nanoid } from 'nanoid'
import { onMounted, ref, useTemplateRef, watch } from 'vue'
import { useI18n } from 'vue-i18n'

import type { ModelMode, ModelRenderer } from '@/stores/model'

import { INVOKE_KEY } from '@/constants'
import { useModelStore } from '@/stores/model'
import { join } from '@/utils/path'
import sprite from '@/utils/sprite'

const dropRef = useTemplateRef('drop')
const dragenter = ref(false)
const selectPaths = ref<string[]>([])
const modelStore = useModelStore()
const { t } = useI18n()

onMounted(() => {
  const appWindow = getCurrentWebviewWindow()

  appWindow.onDragDropEvent(({ payload }) => {
    const { type } = payload

    if (type === 'over') {
      const { x, y } = payload.position

      if (dropRef.value) {
        const { left, right, top, bottom } = dropRef.value.getBoundingClientRect()

        const inBoundsX = x >= left && x <= right
        const inBoundsY = y >= top && y <= bottom

        dragenter.value = inBoundsX && inBoundsY
      }
    } else if (type === 'drop' && dragenter.value) {
      dragenter.value = false

      selectPaths.value = payload.paths
    } else {
      dragenter.value = false
    }
  })
})

async function handleUpload() {
  const selected = await open({ directory: true, multiple: true })

  if (!selected) return

  selectPaths.value = selected
}

watch(selectPaths, async (paths) => {
  for await (const fromPath of paths) {
    try {
      const id = nanoid()

      let mode: ModelMode = 'standard'
      let renderer: ModelRenderer = 'live2d'
      let displayName: string | undefined

      const manifestPath = join(fromPath, 'model.json')

      if (await exists(manifestPath)) {
        const content = await readTextFile(manifestPath)
        let manifest: unknown

        try {
          manifest = JSON.parse(content)
        } catch (error) {
          if (/"renderer"\s*:\s*"sprite"/.test(content)) throw error

          manifest = undefined
        }

        if (manifest && typeof manifest === 'object'
          && 'renderer' in manifest && manifest.renderer === 'sprite') {
          const validatedManifest = await sprite.validateModel(fromPath)

          renderer = 'sprite'
          displayName = validatedManifest.displayName

          if (validatedManifest.mode) {
            mode = validatedManifest.mode
          }
        }
      }

      const files = renderer === 'live2d'
        ? await readDir(join(fromPath, 'resources', 'right-keys')).catch(() => [])
        : []

      if (files.length > 0) {
        const fileNames = files.map(file => file.name.split('.')[0])

        if (fileNames.includes('East')) {
          mode = 'gamepad'
        } else {
          mode = 'keyboard'
        }
      }

      const toPath = join(await appDataDir(), 'custom-models', id)

      await invoke(INVOKE_KEY.COPY_DIR, {
        fromPath,
        toPath,
      })

      modelStore.models.push({
        id,
        path: toPath,
        mode,
        renderer,
        displayName,
        isPreset: false,
      })

      message.success(t('pages.preference.model.hints.importSuccess'))
    } catch (error) {
      message.error(String(error))
    }
  }
})
</script>

<template>
  <div
    ref="drop"
    class="w-full flex flex-col cursor-pointer items-center justify-center gap-4 b-1 b-dashed bg-[--ant-color-fill-quaternary] transition b-border rounded-lg hover:border-primary"
    :class="{ 'border-primary': dragenter }"
    @click="handleUpload"
  >
    <div class="i-solar:upload-square-outline text-12 text-primary" />

    <span>{{ $t('pages.preference.model.hints.clickOrDragToImport') }}</span>
  </div>
</template>
