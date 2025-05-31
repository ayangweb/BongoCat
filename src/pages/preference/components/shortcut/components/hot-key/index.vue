<script setup lang="ts">
import { onClickOutside, useEventListener, useMagicKeys } from '@vueuse/core'
import { ref, useTemplateRef } from 'vue'

import { isModifierKey, normalizeKey } from '@/utils/keyboard.ts'

defineProps<{ hotKey: string }>()
const emits = defineEmits<{ changeShortcut: [shortcut: string] }>()

const isEditing = ref(false)
const pressedKey = ref<string[]>([])
const el = useTemplateRef<HTMLDivElement>('el')

function starEditShortcut() {
  if (isEditing.value) return

  isEditing.value = true
  pressedKey.value = []
}

const { current } = useMagicKeys()
useEventListener('keydown', (event) => {
  if (!isEditing.value) return
  event.preventDefault()
  event.stopPropagation()

  const keys = Array.from(current)
  const modifiers = keys.filter(k => isModifierKey(k))
  const nonModifiers = keys.filter(k => !isModifierKey(k))
  pressedKey.value = [...modifiers, ...nonModifiers].map(k => normalizeKey(k))
})

onClickOutside(el, () => {
  if (!isEditing.value) return

  isEditing.value = false
  emits('changeShortcut', pressedKey.value.join('+'))
})
</script>

<template>
  <div class="bg-gray-1 text-center">
    <div
      v-if="isEditing"
      ref="el"
      class="px-4 py-2"
    >
      {{ pressedKey.length > 0 ? pressedKey.join('+') : '请按下快捷键' }}
    </div>
    <div
      v-else
      class="px-4 py-2 hover:cursor-pointer hover:bg-gray-2"
      @click="starEditShortcut"
    >
      <span>{{ hotKey || 'None' }}</span>
    </div>
  </div>
</template>
