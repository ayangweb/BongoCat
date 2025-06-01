<script setup lang="ts">
import { onClickOutside } from '@vueuse/core'
import { useTemplateRef } from 'vue'

defineProps<{ hotKey: string, isEditing: boolean, pressedKey: string[] }>()
const emits = defineEmits<{ save: [], edit: [] }>()

const el = useTemplateRef<HTMLDivElement>('el')
onClickOutside(el, () => emits('save'))
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
      @click="$emit('edit')"
    >
      <span>{{ hotKey || 'None' }}</span>
    </div>
  </div>
</template>
