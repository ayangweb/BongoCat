<script setup lang="ts">
import { Button, ListItem } from 'ant-design-vue'

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
  <ListItem>
    <span>{{ label }}</span>

    <template #actions>
      <Shortcut v-model="modelValue" />

      <Button
        class="inline-flex items-center justify-center"
        @click="emit('click')"
      >
        <template #icon>
          <div class="i-lucide:play" />
        </template>
      </Button>
    </template>
  </ListItem>
</template>
