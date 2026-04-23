<script setup lang="ts">
import { Button, Divider } from 'antdv-next'

import Shortcut from '@/components/shortcut/index.vue'
import { useKeyPress } from '@/composables/useKeyPress'

const { label } = defineProps<{ label: string }>()
const emit = defineEmits(['click'])
const modelValue = defineModel<string>()

useKeyPress(modelValue, () => {
  emit('click')
})
</script>

<template>
  <div class="flex items-center justify-between px-4 py-2 not-last:(b-b b-b-solid b-border-sec)">
    <span>{{ label }}</span>

    <div class="flex items-center">
      <Shortcut v-model="modelValue" />

      <Divider type="vertical" />

      <Button
        class="inline-flex items-center justify-center"
        @click="emit('click')"
      >
        <template #icon>
          <div class="i-lucide:play" />
        </template>
      </Button>
    </div>
  </div>
</template>
