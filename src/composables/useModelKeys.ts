import { readDir } from '@tauri-apps/plugin-fs'
import { forOwn } from 'es-toolkit/compat'
import { reactive, watch } from 'vue'

import { useCatStore } from '@/stores/cat'
import { useModelStore } from '@/stores/model'
import { isImage } from '@/utils/is'
import { join } from '@/utils/path'

export function useModelKeys() {
  const modelStore = useModelStore()
  const supportKeys = reactive<Record<string, string>>({})
  const pressedKeys = reactive<Record<string, string>>({})
  const catStore = useCatStore()

  watch(() => modelStore.currentModel, async (model) => {
    if (!model) return

    const resourcePath = join(model.path, 'resources')
    const groups = ['left-keys', 'right-keys']

    for await (const groupName of groups) {
      const groupDir = join(resourcePath, groupName)
      const files = await readDir(groupDir).catch(() => [])
      const imageFiles = files.filter(file => isImage(file.name))

      for (const file of imageFiles) {
        const fileName = file.name.split('.')[0]

        supportKeys[fileName] = join(groupDir, file.name)
      }
    }
  }, { deep: true, immediate: true })

  const handlePress = (key: string) => {
    if (catStore.singleMode) {
      forOwn(pressedKeys, (_, key) => {
        delete pressedKeys[key]
      })
    }

    const path = supportKeys[key]

    if (!path) return

    pressedKeys[key] = path
  }

  const handleRelease = (key: string) => {
    delete pressedKeys[key]
  }

  return {
    supportKeys,
    pressedKeys,
    handlePress,
    handleRelease,
  }
}
