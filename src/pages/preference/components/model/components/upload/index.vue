<script setup lang="ts">
import type { ModelMode } from '@/stores/model'

import { invoke } from '@tauri-apps/api/core'
import { appDataDir } from '@tauri-apps/api/path'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { open } from '@tauri-apps/plugin-dialog'
import { readDir } from '@tauri-apps/plugin-fs'
import { message } from 'ant-design-vue'
import { nanoid } from 'nanoid'
import { onMounted, ref, useTemplateRef, watch } from 'vue'

import { INVOKE_KEY } from '@/constants'
import { useModelStore } from '@/stores/model'
import { join } from '@/utils/path'

const dropRef = useTemplateRef('drop')
const dragenter = ref(false)
const selectPaths = ref<string[]>([])
const modelStore = useModelStore()

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

      const files = await readDir(join(fromPath, 'resources', 'right-keys')).catch(() => [])

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
        isPreset: false,
      })
      // 导入成功 Imported successfully
      message.success('Nhập khẩu thành công')
    } catch (error) {
      message.error(String(error))
    }
  }
})
</script>

<template>
  <div
    ref="drop"
    class="w-full flex flex-col cursor-pointer items-center justify-center gap-4 b b-color-1 rounded-lg b-dashed bg-color-8 transition hover:border-primary"
    :class="{ 'border-primary': dragenter }"
    @click="handleUpload"
  >
    <div class="i-solar:upload-square-outline text-12 text-primary" />
    <!-- 点击或拖动至此区域导入 Click or drag to this area to import -->
    <span>Nhấp hoặc kéo vào khu vực này để nhập</span>
  </div>
</template>
