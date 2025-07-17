import { readDir } from '@tauri-apps/plugin-fs'
import { computed, ref, watch } from 'vue'

import { useModelStore } from '@/stores/model'
import { isImage } from '@/utils/is'
import { join } from '@/utils/path'

export function useKeys() {
  const modelStore = useModelStore()
  const supportLeftKeys = ref<string[]>([])
  const supportRightKeys = ref<string[]>([])
  const pressedLeftKeys = ref<string[]>([])
  const pressedRightKeys = ref<string[]>([])

  const supportKeys = computed(() => {
    return [...supportLeftKeys.value, ...supportRightKeys.value]
  })

  const pressedKeys = computed(() => {
    return [...pressedLeftKeys.value, ...pressedRightKeys.value]
  })

  watch(() => modelStore.currentModel, async (model) => {
    if (!model) return

    const keySides = [
      {
        side: 'left',
        supportKeys: supportLeftKeys,
        pressedKeys: pressedLeftKeys,
      },
      {
        side: 'right',
        supportKeys: supportRightKeys,
        pressedKeys: pressedRightKeys,
      },
    ]

    for await (const item of keySides) {
      const { side, supportKeys, pressedKeys } = item

      try {
        const files = await readDir(join(model.path, 'resources', `${side}-keys`))

        const imageFiles = files.filter(file => isImage(file.name))

        supportKeys.value = imageFiles.map((item) => {
          return item.name.split('.')[0]
        })

        pressedKeys.value = pressedKeys.value.filter((key) => {
          return supportKeys.value.includes(key)
        })
      } catch {
        supportKeys.value = []
        pressedKeys.value = []
      }
    }
  }, { deep: true, immediate: true })

  return {
    supportLeftKeys,
    supportRightKeys,
    pressedLeftKeys,
    pressedRightKeys,
    supportKeys,
    pressedKeys,
  }
}
